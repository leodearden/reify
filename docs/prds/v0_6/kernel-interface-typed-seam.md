# PRD — Kernel interface: typed replies, total delegation, one selector

**Status:** authored pass 1, 2026-10-13 (review-all program `review-all-reify-20261010`, stream **S10**, wave 2, spawn mode, size L, risk high; program contract `plans/review-all-program-reify-20261012.md`). Leo ruled R1–R6 in-session (§3). An adversarial critic pass (fable/high: 4 blockers, 10 majors, 10 minors) is folded (lead decisions L13–L17 and premise corrections 11–14 are its outcomes). Decomposed 2026-10-14: α₁ #8930, α₂ #8931, α₃ #8932, α₄ #8933, α₅ #8934, β₁ #8935, β₂ #8936, β₃ #8957, γ #8958, δ₁ #8959, δ₂ #8960, δ₃ #8961, δ₄ #8962, δ₆ #8963, δ₇ #8964, ε₁ #8965, ε₂ #8966, ε₃ #8967, ε₄ #8968, ε₅ #8969, ζ₁ #8970, ζ₂ #8971, ζ₃ #8972, ζ₄ #8973, η₁ #8974, η₂ #8975, η₃ #8976, η₄ #8977, η₅ #8978, η₆ #8979, θ₁ #8980, θ₂ #8981, θ₃ #8982, θ₄ #8983, θ₅ #8985, ι₁ #8987, ι₂ #8989, ι₃ #8992, ι₄ #8993, ι₅ #8995, ι₆ #8998, ω #9000 (close). Subsumed and cancelled: #8532, #8790, #8795, #6137, #6474 and S14 κ #8632. v0_6. Approach **B + H** (multi-kernel is a load-bearing seam per the overlay's G5 list): contracts in §5, boundary tests facing both ways in §5's tables.
**Invariants established or corrected:** **INV-GEO-5** (new; program invariant candidate 6: a delegating `GeometryKernel` wrapper forwards every capability, by type plus a delegation lint; partial at ε₂, enforced at ε₅). **INV-GEO-1** (mechanism made true: the mesh-consuming capability methods take `&ValidatedMesh`; leaf ζ₂). **INV-GEO-2** (consumer-side clause added: a handle is served only by its owning kernel; leaf ι₅). **INV-GEO-4** (corrected now in the landing commit, since its text is inverted at main; completed by leaf δ₄). INV-GEO-3 is **not** S10's: S11 ε #8682 owns its OCCT rewording (§7). Every leaf that extracts a shared primitive names its row in `tests/infra/adoption-ratchet.manifest` (invariant candidate 5). Each flip writes resolving pointers in S24's pointer grammar (`reify-audit-typed-findings-and-registry-lanes.md` §5.7): in the Enforcement cell before S24 ξ #8821 lands, in the Pointer column after it.
**Finding keys:** 23 owned: the 21 keys in the stream row, fk-030602c2fd62 reassigned from S11, and the prior-instrument canonical `language-pipeline|crates/reify-ir/src/geometry.rs::mesh|h10` (prior key fk-a60cf0fbe772, p10). Nineteen go to leaves here, one of them (fk-0f933e75bf7d) through the subsumed filed task #8532. Three stay on filed tasks that are referenced, not re-filed (#8533, #8529, #8561). One (fk-92d313d2986f) is stamped onto meshing-service β #8285 (R4). β₁ also carries S14's fk-c497935d4b8a when it subsumes S14 κ #8632 (L13). §11 maps every key.
**Code anchors** verified against main `274a212e0a` (2026-10-13); at commit time main is `cf59570562`, where **#6308 has landed** (export and terminal-owner routing, `kernel_owner.rs`). The leaves that depend on #6308 (ζ₂, ζ₄, ι₃) start from its tree. All other S10 paths are unchanged. `git diff a030ee7528..main` over every S10 path is empty except +4 lines in `crates/reify-kernel-gmsh/src/kernel_real.rs` (a `cfg_attr` on `TetReadback::tag_to_idx`), so the review-run anchors hold. In-flight branches read: `task/8533`, `task/8529`, `task/8652`, `task/6308`. Main moves fast: cite by symbol and re-locate lines at implementation time. Counts carry their rule inline.

---

## 1. Consumers and user-observable surface (G1, G2)

**Consumers.**
- **Design authors**, through four behaviours that are wrong today. Each was measured with `target/debug/reify` (built 2026-10-12) on main `274a212e0a`:
  - `scale(base, 2mm)` silently scales by 0.002: `ScaledBlock.v_bad = 0.000000000008 m^3`, exit 0, no diagnostic. Fixture: `tests/prd-gate/fixtures/kernel_interface_scale_dimensioned_factor.ri`. Leaf θ₅.
  - `linear_pattern(base, 1, 0, 0, 3000mm, 20mm)` reads the count `3000mm` as 3 copies: `Pat.vp = 0.0000029999999999999997 m^3`, exit 0. Fixture: `tests/prd-gate/fixtures/kernel_interface_pattern_dimensioned_count.ri`. Leaf θ₅.
  - `half_space(0mm, 0mm, 0mm, 0, 0, true)` fails with `error: geometry error: geometry operation failed: expected numeric value`. The error names no op, no argument and no span. Fixture: `tests/prd-gate/fixtures/kernel_interface_halfspace_nonnumeric_normal.ri`. Leaf θ₁.
  - `reify build <union of two overlapping boxes> -o x.stl` exits 1 with `error: internal error: dispatcher named target kernel 'manifold' not present in engine.geometry_kernels for a conversion plan`. It still writes an STL, and that STL holds only `box_b` (12 triangles, x in [0, 10] mm). The dispatcher plans over the process-global registry, which lists kernels this Engine never instantiated. Fixture: `tests/prd-gate/fixtures/kernel_interface_boolean_stl_export.ri`. Leaf ι₄ (with #6104).
- **The next agent to change the kernel seam.** Every structural finding here is a next-change cost:
  - adding a trait capability (nine forwarders went missing in the holder, and three test proxies mask eleven methods each);
  - adding a query reply (the shape lives in doc prose, and five tokenizers re-parse JSON);
  - adding an op with a length field (`Value` or `f64`, with no rule);
  - adding a kernel or conversion (four selection mechanisms, two map-key schemes, a silent default fallback);
  - adding a mesh consumer (the witness is never demanded);
  - adding a kernel test double (79 hand-written impls).
  §2 states the cost per finding.
- **Downstream program streams.** S1 (wave 3) consumes `KernelSet` (ι₃) as its single constructor's kernel argument. S2 rewrites `engine_build.rs` after ι₃–ι₅ have moved its kernel lookups behind `dispatcher::select`. S24 ξ #8821 records this stream's INV-GEO owner leaves (§7). meshing-service β #8285 carries fk-92d313d2986f (R4), and ε #8288 deletes the `VolumeMeshing` sub-trait that ε₄ creates.

**In-engine seams (overlay G1 sub-check).** No new seam. ε, ζ, η and θ reshape §3.1 op-execute (`GeometryKernel` and `GeometryOp`). ι reshapes §3.3 multi-kernel dispatch (`dispatcher.rs`). ζ₂ reshapes the §3.2 conversion handoff (`Engine::execute_realization_ops`).

**Leaf signals (G2).** The user-visible leaves above name a CLI observation. Refactor leaves follow S4's precedent and name three things:
- (a) a deletion or ratchet signal that turns red on the superseded pattern;
- (b) a behaviour pin on a real surface: a CLI oracle that must stay byte-identical (`tests/prd-gate/fixtures/kernel_interface_query_reply_oracle.ri` for the reply rollout, plus existing examples named per leaf);
- (c) for contract leaves, a shared contract test instantiated on every real kernel.

No leaf's only signal is a unit test over synthetic input.

## 2. Findings this stream owns (record `plans/review-all-reify-20261010.json`)

| key | anchor | sev | verdict | short | cost of the next change | leaf |
|---|---|---|---|---|---|---|
| fk-047cb467bd07 | `reify-ir/src/geometry.rs::GeometryKernel` | **high** | confirmed | f210 | 27 methods, 23 defaulted. A wrapper that misses a forwarder compiles and answers "not supported"; this happened nine times in the holder. `CapabilityDescriptor.supports` is a second, unchecked encoding of the same capabilities. | ε₅ (ε₁–ε₄ deliver the parts) |
| fk-ef5f5da8f729 | `reify-geometry/src/lib.rs::SingleKernelHolder` | medium | confirmed | f84 | The holder forwards 18 of 27 methods, and its parity test lists those same 18. | **filed #8533** (in progress: 27 forwards plus a syn-enumerated guard) |
| fk-a01d863c73ba | `reify-ir/src/geometry.rs::ValidatedMesh` | medium | confirmed | f211 | Four public entry points, zero signatures demanding the witness, both production `validate` callers discard it, and a 343-line `check_contract`. | ζ₂ (ζ₁ first) |
| fk-a60cf0fbe772 (p10) | `reify-ir/src/geometry.rs::mesh` (prior canonical) | — | still_present | p10 | The same defect seen by the prior instrument: no type enforcement at the seam, and OpenVDB re-implements a partial IndexValid check. | ζ₂ |
| fk-1d857a1e9750 | `reify-ir/src/geometry.rs::GeometryOp` | medium | confirmed | f212 | Quantities travel as `Value` (47 fields) or bare `f64` (33 length/angle fields). The decode macro has zero invocations, and two kernels carry hand-copied extractors. | θ₄ (θ₁–θ₃ first) |
| fk-fdb4e221350a | `reify-ir/src/geometry.rs::GeometryQuery` | medium | confirmed | f213 | All 35 replies are untyped `Value`. Shapes live in prose (9 variants state none; one states the wrong one). The kind vocabularies are copied 3 times in code. | η₆ |
| fk-59c46755d24c | `reify-eval/src/topology_selectors.rs::parse_flat_number_object` | **high** | confirmed | f69 | 5 independent JSON tokenizers (4 eval, 1 test-support) and 8 wrappers re-parse point/bbox replies that the FFI already had as structs. | η₃ |
| fk-0375628ba585 | `reify-eval/src/selector_vocabulary_v2.rs::extremal_by_bbox` | medium | confirmed | f81 | Structure is serialized to text and re-parsed in one process. The bbox axis is chosen by a `b'x'` byte tag. | η₃ |
| fk-030602c2fd62 | `reify-kernel-occt/src/lib.rs::OcctKernel.query` | medium | weakened | f66 | 7 OCCT JSON-string reply arms plus 2 kind-name arms. Reassigned from S11 (S11 Q1 → (a)): the producer and the eval parsers switch atomically, per reply family. | η₃ (η₂ and η₄ deliver the other families) |
| fk-66c1d2fe9ff9 | `reify-ir/src/geometry.rs` | medium | confirmed | f328 | The SCC {boundary_attachment, expr, geometry, value}. geometry.rs is 13,364 lines holding two strata. | α₁ (α₂, α₃ do the h14 half) |
| fk-80c2f82c17d6 | `reify-eval/src/dispatcher.rs::dispatch` | medium | confirmed | f334 | Four selection mechanisms and 42 kernel-map lookups (rule §4.6). Two map-key schemes, and every production front end runs in sentinel mode. A handle from a non-default kernel is served by OCCT silently. | ι₆ (ι₂–ι₅ first) |
| fk-e6a266a72f41 | `reify-kernel-fidget/src/kernel.rs::FidgetKernel` | medium | weakened | f86 | Only OCCT instantiates the shared contract. The gmsh and openvdb uses sit in `not(has_*)` modules that no gate compiles. | δ₂ |
| fk-107e28de9029 | `reify-kernel-occt/src/stubs.rs::OcctKernel` | medium | weakened | f85 | The stub surface is compiled by no gate step. **R5: the stub arms are removed**, not gated. | β₁ |
| fk-92d313d2986f | `reify-kernel-gmsh/src/init.rs::lock` | medium | weakened | f51 | Four entry points restate a six-step session preamble. | **stamped onto meshing-service β #8285** (R4); ζ₃ moves the element-order tables |
| fk-a0ce1853f2da | `reify-kernel-manifold/src/kernel.rs::manifold_from_reify_mesh` | low | weakened | f345 | Two homes for the mesh-contract posture; Manifold keeps a process-global `OnceLock`. | ζ₂ |
| fk-57ffd6bd2af2 | `reify-compiler/src/module_pragmas.rs::KNOWN_V02_KERNELS` | low | weakened | f323 | A literal kernel-name set beside `KernelId::ALL`, plus two registry-name bridges with different unknown-name policies. | ι₁ |
| fk-08aa3fff3378 | `reify-ir/src/kernel_validation.rs::check_length_field` | medium | confirmed | f246 | A second classifier and label family, justified by a rationale that has been false since #5791, with stale line pointers. | θ₃ |
| fk-7b093573458b | `reify-ir/src/value.rs::StructureInstanceData` | medium | weakened | f214 | Engine overlays stored under `@@` keys in the public `fields` map and filtered by a private `user_fields()`. | α₅ |
| fk-448317051ff9 | `scripts/assert-crate-dag.sh` | medium | weakened | f329 | The gating crate DAG covers 4 of 37 packages; kernel, solver, analysis and test-support strata are unchecked. | γ |
| fk-65bb1ffb5d60 | `reify-kernel-occt/src/projector_impl.rs` | low | confirmed | f331 | An optional upward occt → mesh-morph edge that exists for one test-only impl. | **filed #8561** (referenced; γ lists it as a named strata exception) |
| fk-10ec3d2a1b76 | `reify-kernel-manifold/src/kernel.rs::STUB_MSG` | medium | confirmed | f50 | The catch-all message misstates the kernel. | **filed #8529** (in progress) |
| fk-0f933e75bf7d | `reify-kernel-openvdb/src/lib.rs` | low | confirmed | f80 | The crate doc says the real FFI is deferred. | β₁ (**subsumes filed #8532**: after the stub arm is deleted the crate has one arm, and β₁ rewrites the doc) |
| fk-7bfef234d79e | `reify-test-support/src/mocks.rs` | medium | confirmed | p14 | 79 concrete impls (5 real, 3 stub, 1 holder, 70 test doubles). MockGeometryKernel runs against no contract, and reify-kernel-conformance instantiates no suite. | δ₇ |

**Premise corrections found at main** (verified by the seats and spot-checked by the lead). These are G6 inputs: leaves are built on the corrected premise.
1. **Two of the measured user-visible failures are already filed.** `isosurface(box)` exporting a 2-triangle STL is **#6308**, in progress: the export path hands an openvdb handle id to OCCT. Seat E's open-mesh readings (box 2 triangles, cylinder 58, all open) came through that same export path, so they say nothing about the MarchingCubes producer. Whether MarchingCubes output satisfies the contract is **unmeasured**: #6308's branch asserts triangle counts and a bounding box, not closedness. It is ζ₄'s first step. The boolean STL failure is **#6104**.
2. **The Gmsh `Mesh→VolumeMesh` and OpenVdb `Mesh→Voxel` conversions are already descriptor rows.** The hard-coded `KernelId::Gmsh`/`OpenVdb` arms bypass rows that exist. The descriptor extension must add queries, exports and projections (densify). It must not add conversions (fk-80c2f82c17d6 correction).
3. **OCCT executes `TransformAffineApply` and `SurfaceNurbs` but declares neither.** They run only through the sentinel None-arm escape: `git grep 'TransformAffineApply\|SurfaceNurbs' crates/reify-kernel-*/src/register.rs` returns nothing. Re-keying the map by `KernelId` would turn `affine_apply`/`nurbs_surface` into NoKernelChain errors unless ι₂ lands first.
4. **INV-GEO-4's text is inverted.** The `has_occt` real arm is wired (`ca22b761d8`, 2026-07-10, `assert_kernel_contract!(real; …)` in `reify-kernel-occt/src/lib.rs`). The stub arm the row calls its enforcement runs in no gate.
5. **`reify_build_utils::find_dir_with_override` takes a non-empty `*_LIB_DIR` on trust, and an empty one falls through to the real library**, so no in-tree mechanism can select a `not(has_*)` arm on a host that has the dependency. This is why R5 removes the stub arms rather than gating them. S14 κ #8632's "empty `GMSH_LIB_DIR` compiles the stub arm" signal cannot work as worded (§12).
6. **S6 β #8829 / γ #8830 add new reify-ir files (`joint_kind.rs` and a RelationKind module) and edit only `lib.rs`.** They do not touch `geometry.rs` or `value.rs`, so no S10 leaf depends on them. The program cross-reference "S10 consumes JointKind/RelationKind" has no S10 consumer site; `lib.rs` overlap is lock-level only (§12).
7. **S7 π #8661 covers only reify-compiler's nine `types.rs` enums**, so `FaceSurfaceKind`/`EdgeCurveKind` derives are S10's (η₄).
8. **cxx shared enums already cross the OCCT bridge** (`ffi.rs` `StepGuardFault`, `StepReadStatus`). The `GeometryQuery::FaceSurfaceKind` doc's "cxx cannot share enums" is false.
9. **No production code produces the JSON reply format in reify-eval.** All 13 eval-side `{"x":`/`xmin` format sites sit in `#[cfg(test)]` modules: they are mock replies.
10. **The reify-ir SCC does not fully dissolve.** Moving the identifier vocabulary leaves `{expr, value}`, a genuine type recursion through `Value::Closure { body: CompiledExpr }`. Simulated with `scripts/assert-module-scc.py --root <scratch copy>`: the row becomes `[[completeness, ranked], [expr, value]]`. The record's `impl From<MeshContractViolation> for GeometryError` cannot close the mesh↔error cycle, because the bridge takes a `kernel` argument.
11. **A 28th trait method always lands before ε₁.** meshing-service α #8284 adds a **defaulted** `GeometryKernel::tessellate_with_topology`, and S11 θ #8672 depends on #8284 (`get_task 8672`: dependencies 8284, 8648, 8652), so ε₁ (which needs θ) always sees it. ε₁ therefore moves it into a twelfth sub-trait unconditionally. Counts below say 27 methods at main and 28 after #8284.
12. **Owner-routed queries are already filed.** Implicit-solids γ **#8391** (pending, high; its only dependency #8388 is done) delivers owner-routed-or-rejected geometry, kinematic and conformance queries, with `DiagnosticCode::QueryKernelMismatch` / `QueryKernelUnserved` and five delivered checks; its B14 fixture `tests/prd-gate/fixtures/implicit_solids_owner_routing_isosurface.ri` is the voxel-query case. **#6366** (pending, low) owns `distance_between_placed`, the declarative `: Output` export and `resolve_export_body_color`, and implicit-solids υ #8399 depends on it. **#6137** (pending) asks for a union exported under `with_registered_kernels` plus a `make_compound` story for a Mesh-only owner. Its ExportBody half is #6308; the rest is ι₅ (L14).
13. **No consumer reads a mock's `BRepKind`.** `git grep -c BRepKind -- crates/reify-eval/src` returns nothing. A MockGeometryKernel that mirrors S11 ζ's `ReprPolicy` therefore has no consumer (G1), and the program cross-reference "MockGeometryKernel mirrors ReprPolicy" is not built (§12).
14. **R5 reaches beyond the kernel crates.**
    - Four non-kernel build scripts emit native cfgs themselves: `crates/reify-cli/build.rs` (has_openvdb), `crates/reify-eval/build.rs` (has_openvdb, has_gmsh), `crates/reify-mesh-morph/build.rs` (has_gmsh) and `gui/src-tauri/build.rs` (has_openvdb).
    - reify-kernel-gmsh and reify-kernel-openvdb carry a `stub_register` feature that gates `inventory::submit!`.
    - The GUI's `get_kernel_status` Tauri command reads `OCCT_AVAILABLE` (`gui/src-tauri/src/kernel_status.rs`).

## 3. Rulings (Leo, 2026-10-13, this session) and lead decisions

**Program rulings this stream implements** (§Resolved decisions, S10): capability sub-traits behind Option-returning accessors (choice 1); GeometryOp quantity fields decoded once into typed SI newtypes built only through the eval-layer gate (choice 2); Mesh-consuming trait methods take `&ValidatedMesh`; typed GeometryQuery replies; one kernel-selection function in `dispatcher.rs` with `geometry_kernels` keyed by `KernelId` and `CapabilityDescriptor` extended; contract suite split into a neutral core plus a B-rep extension; `KNOWN_V02_KERNELS` derived from `KernelId`, not deleted; crate DAG extended to kernel, solver, analysis and test-support strata; h14 partitions of `geometry.rs` and `value.rs`.

**Leo's rulings on this stream's open questions (2026-10-13):**
- **R1 — Seams.** S10 edits only the `GeometryKernel` (and sub-trait) impl blocks and the signature-following hunks in `reify-kernel-occt/src/handle.rs`, ordered after S11 θ #8672. S11 keeps the inherent `OcctKernelHandle` surface. (After β₁ deletes `stubs.rs`, no stub edit remains.) S10 also defines `KernelSet` (keyed by `KernelId`, no Engine constructor signature change) and takes the build-routing `collect_registry()`/`registry()` reads from S1's fk-4db43199eda2. S1 keeps the link anchors, its single constructor (which consumes `KernelSet`) and the `test_registry_override` deletion.
- **R2 — SI newtypes.** `Length::accept(&Value)` and `Angle::accept(&Value)` are the eval gate's own check (`reify_ir::arg_acceptance::accept_arg` with `length_spec`/`angle_spec` plus finiteness; S15's frozen code is unchanged). A raw `Length::from_metres(f64)` / `Angle::from_radians(f64)` serves engine-internal SI data, and its production call sites are pinned by an adoption-ratchet allowlist. This amends the ruling's literal "built only through the eval-layer gate", which Rust cannot enforce across crates. Scope: the 47 `Value` quantity fields **and** the ~33 bare-`f64` length/angle fields, plus a guard that every remaining bare numeric `GeometryOp` field carries a justification (direction, factor, weight, knot, count, degree, index, flag).
- **R3 — Mesh witness.** `ValidatedMesh` carries the posture it was minted under. `Finite` and `IndexValid` are never waivable, because OpenVDB and gmsh read meshes in C++. `REIFY_MESH_CONTRACT=warn` waives only NonDegenerate, Closed and ConsistentWinding, and the waiver travels in the witness. Manifold's process-global `OnceLock` goes. **Five of the six** mesh-consuming methods take `&ValidatedMesh`. `measure_mesh_deviation` keeps `&Mesh`: it samples the kernel's own fresh tessellation and is not an ingest. This amends the ruling's "Mesh-consuming trait methods" to name the five.
- **R4 — gmsh session.** fk-92d313d2986f is stamped onto meshing-service β #8285. β's C4 step 1 is this session, including the ElementOrder, Algorithm3D and NumThreads option scope; a note names the ratchet rows. S10 keeps one small leaf (ζ₃) moving the element-order degree and tet-stride tables into reify-ir. gmsh's four volume-meshing trait methods are treated as born-then-deleted by ε #8288 (cross-references only).
- **R5 — The OCCT/gmsh/OpenVDB-free build is removed, not supported.** Leo asked why it should be supported. No live consumer was found:
  - there is no wasm crate or cfg;
  - "OCCT-free" in the docs means a crate whose dependency closure excludes OCCT, which is unaffected;
  - setup-dev.sh builds the deps, and check-manifold-deps.sh already fails every verify without them;
  - the stub arms date from a March 2026 "no-OCCT fallback".

  So: `build.rs` fails closed when a native dependency is missing, and the three stub arms with their dark tests are deleted (β₁). The always-true `cfg(has_occt|has_gmsh|has_openvdb)` gates (about 1,100 lines in about 225 files) and the `*_AVAILABLE` guards (652 references) are then swept, with a ratchet to 0 (β₂, β₃). S14 κ #8632 then has no stub to serve (cross-reference to the lead, §12).
- **R6 — #6366 is split (2026-10-14).** ι₅ takes part (1), `distance_between_placed`: the owner serves it, and mixed owners give a coded error. #6366 keeps parts (2) and (3), the declarative `: Output` export and `resolve_export_body_color`, which need `Value::GeometryHandle` to carry its kernel. #6366 depends on ι₅ and builds on `select`. Implicit-solids υ #8399 therefore waits for ι₅ (L14).

**Lead decisions** (design-level, decided under the rulings, recorded so no leaf re-decides them):
- **L1.** Accessors are **required**: there are no default bodies anywhere on `GeometryKernel`. `no_kernel_capabilities!()` and `forward_kernel_capabilities!(field)`, written once beside the trait, keep a capability-free mock to one line. A workspace `syn` lint checks every delegating impl, meaning one whose body forwards a required core method to a field. Such an impl must either invoke `forward_kernel_capabilities!(<that field>)` or have every accessor body forward to the same field. Any other accessor body in a delegating impl is red, including a hand-written `None`. An accessor can be exempted only by an annotated `// INV-GEO-5 masked: <reason>` line, which is counted with a ratchet at 0. Omission is a compile error; the lint closes the opt-out and the hand-written-`None` loopholes (h10). `reset` becomes a required core method, so the Manifold, OpenVDB, Fidget and Gmsh table retention documented on `reset` becomes an explicit per-kernel decision.
- **L2.** `execute_with_history`, `query_many` and `export_with_options` become capabilities (`OpHistory`, `BatchQuery`, `OptionedExport`), each with one fallback in a reify-ir facade (`kernel_calls`, free functions over `&dyn`/`&mut dyn GeometryKernel`). The facade owns the one "capability absent" error per method (h11). The `export_with_options` fallback returns an `ExportWarning` naming the ignored options instead of silently dropping them (INV-SF-3).
- **L3.** `closest_point_on_shape` duplicates `GeometryQuery::ClosestPointOnShape`. Once η₂ types that reply (a `Point`), ε₅ deletes the trait method and mesh-morph's `KernelProjector` uses the query. The probe sub-trait keeps `vertex_point` and `measure_mesh_deviation`.
- **L4.** MockGeometryKernel does **not** mirror S11 ζ's `ReprPolicy`. reify-eval reads `BRepKind` nowhere (premise correction 13), so the mirror would have no consumer (G1). It is recorded as a follow-up candidate, to be built if a consumer appears, and as a cross-reference to the lead (§12).
- **L5.** INV candidate 6 is the new row **INV-GEO-5** ("a delegating wrapper forwards every capability"), not a wider INV-GEO-4: one row, one failing thing. Test decorators route through one shared `ObservingKernel<K>` (δ₆), so the lint has one delegator to check.
- **L6.** The dangling-handle contract is **strict** where a path is supported: every handle-taking path returns its own enum's invalid-handle variant (`GeometryError::InvalidReference`, `QueryError::InvalidHandle`, `ExportError::InvalidHandle`, `TessError::InvalidHandle`). Fidget's query/export, Manifold's three query arms plus extract and export, and OpenVDB's execute/tessellate change to match (δ₃). Fidget's own doc schedules exactly this once meshing lands, and it has.
- **L7.** Query replies use a typed `QueryReply` enum returned by the trait, converted to `Value` at one eval point (`geometry_ops/query.rs::reply_to_value`). Rollout is expand/contract, with a counted transitional `QueryReply::Untyped(Value)`; each reply family switches its producer and parsers in one leaf (η₂–η₅). The FFI kind string is decoded once in Rust. Turning the C++ switch into a cxx shared enum is a follow-up candidate for S11. `normal()` keeps surfacing `Value::Vector`, byte-identical.
- **L8.** `KernelSet` keys an injected kernel (`Engine::new(_, Some(k))`, `with_prelude`) as `KernelId::Occt` with a typed `Declared::Injected` posture that serves any op, which is today's sentinel behaviour typed. Descriptor rows for queries, exports and projections are **declared and cross-checked** against the accessors by a registry test, not derived (D7 in §4). Users observe the serving kernel through coded diagnostics on absent owners and through the leaves' CLI signals; no new `--verbose` line is added, because that would touch S1's BuildResult partition (follow-up candidate).
- **L9.** The `geometry.rs` move goes after #8533 (whose guard `include_str!`s the file), #8284 and #7379, all of which edit the file, and not after the rest of the meshing-service chain. The move preserves every `reify_ir::geometry::X` path through a facade module. **No leaf writes task records.** The decompose session widens, before filing α₂/α₄, the delivered checks of other tasks that pin a moved file: #8284's `fn tessellate_with_topology\b` and #8686/#8711/#8717's `value.rs` checks. They are widened to `crates/reify-ir/src`, in the three writes (sidecar `.yaml`, `.md` twin, task metadata). Pending tasks whose `metadata.files` name a moved path are left as they are: an under-declared lock is the safe direction (overlay), and their architects acquire the real footprint.
- **L10.** `{expr, value}` stays as a recorded INV-ARCH-1 residual (a type recursion, not a layering defect). The crate-strata table (γ) is the authority for crates `.jcodemunch.jsonc`'s PLAYER does not assign. PLAYER gets a pointer comment and is not extended. No new registry row is added for strata: the existing crate-DAG assertion has none, and γ extends that assertion.
- **L11.** The materialized-annotation overlay, written in production and read only by tests, is kept and typed (α₅). Deleting it is a design call for the `AtMaterialization` owner and is recorded as a follow-up candidate. #7867 (remove `type_id`) lands before α₅, since both rewrite the same construction literals.
- **L12.** One ratchet engine: S10's extraction rows live in `tests/infra/adoption-ratchet.manifest` (on main; `tests/infra/adoption_ratchet.py`). Structural guards that need a parse live as crate tests: ε₂'s delegation lint, θ's GeometryOp field guard and η₆'s reply-kind contract. No new `tests/infra` member is added, so no `run-all-classification.manifest` row is needed. **Row rules** (the engine reads Python `re` per line, expands paths with `pathlib.Path.glob`, and counts `//` lines as code in Rust files):
  - globs are `**/*.rs`, because `src/*.rs` is non-recursive and misses `compute_targets/` and the post-S13 `geometry_ops/` children;
  - a pattern must match on one line;
  - a pattern is prefixed `^[^/]*` so it cannot match `//` comment text;
  - each leaf measures its own row's baseline with `python3 tests/infra/adoption_ratchet.py` at its base commit and records it in the commit message;
  - one pattern-id per glob, because the engine rejects duplicate ids: rows that cover several crates take a `-<crate>` suffix.

- **L13.** **β₁ subsumes S14 κ #8632.** This supersedes the program's recorded S14 Q2 → (A) ruling ("κ adds BackendUnavailable in wave 1; S10 consumes it"), as a consequence of R5. R5 removes κ's only producer (the gmsh stub arm) and its substring routing. κ is cancelled at decomposition with a reason naming β₁, after a dependents check; any dependent is rewired to β₁. Its key fk-c497935d4b8a moves onto β₁. A note is appended to meshing-service β #8285: the marker is already gone when it lands.
- **L14.** **ι₅ builds on implicit-solids γ #8391.** ι₅ depends on #8391 and reuses its `QueryKernelMismatch` / `QueryKernelUnserved` codes; it mints no new code. It re-expresses #8391's owner check through `select(OnHandle)` over `KernelSet` and keeps the constructs #8391's delivered checks grep for (`.with_code(DiagnosticCode::QueryKernelMismatch)`, `OwnerMismatch`). **#6366 is split.** Its site (1), `distance_between_placed`, is a handle-served request, and ι₅ routes it through `select(OnHandle)`. When both bodies have one owner, that owner serves the query; mixed owners give a coded Error, which is the honest error #6366 asked for. Sites (2), the declarative `: Output` export, and (3), `resolve_export_body_color`, need `Value::GeometryHandle` to carry its kernel, an IR-level design that stays #6366's. **#6366 gains a dependency on ι₅** and builds (2)/(3) on `select`. #6308's helpers, which #6366's text prescribes, are gone by then. The decompose session amends #6366's details accordingly, and implicit-solids υ #8399 (which depends on #6366) waits for ι₅ (§12). **#6137 is subsumed by ι₅**: its union-under-`with_registered_kernels` test and its Mesh-only-owner `make_compound` decision are ι₅ work. It is cancelled at decomposition, after its content is ported.
- **L15.** **#6104 depends on ι₄.** ι₄ creates `crates/reify-cli/tests/harness_cli/cli_build_boolean_stl.rs`, and #6104 extends it. #6104 rejected "fall back in the conversion-plan arm" because the arm fell back blindly. ι₄ does something different: it plans over the descriptors of the kernels the engine holds, so a plan never names an absent kernel, and there is nothing to fall back from.
- **L16.** The MarchingCubes question becomes its own leaf, **ζ₄**, which measures first. If MarchingCubes output fails the contract, ζ₄ stops and escalates the measured witness as a premise blocker. No leaf files a task.
- **L17.** Born-then-deleted surfaces under meshing-service ε #8288 are accepted and noted where they occur: the `VolumeMeshing` sub-trait (ε₄), gmsh's `kernel_contract.rs` (δ₂), gmsh's `register.rs` rows (ι₆), the `is_pragma_target` gmsh arm (ι₁) and the element-order tables' gmsh call sites (ζ₃). Each is small, and ε #8288 deletes whole items rather than editing around them. **If #8288 lands first**, each affected leaf drops its gmsh part:
  - ε₁/ε₄ do not create `VolumeMeshing` for deleted methods;
  - ζ₂'s typed signatures are whichever of the five still exist, and its INV-GEO-1 cell lists only those;
  - δ₂ skips the gmsh `kernel_contract.rs`, and δ₄'s INV-GEO-4 row text omits the gmsh pointer;
  - ι₆ skips gmsh's `register.rs`;
  - ι₁'s predicate has no gmsh arm, so the derived set is all of `KernelId::ALL`;
  - ζ₃ skips the deleted gmsh call sites.

  **If S10 lands first**, #8288's note asks it to delete the gmsh `kernel_contract.rs` and remove the gmsh pointer from INV-GEO-4 in the same diff, and to delete the `VolumeMeshing` sub-trait and accessor whole. The gmsh stub `kernel.rs` listed in #8288's amendment is already gone after β₁.

## 4. Sketch of approach — the target structure

```
crates/reify-ir/src/
  geometry_id.rs          GeometryHandleId, KernelHandle (+pub use KernelId), FeatureId family, AxisSign,
                          CapKind, ModEntry, Role, FaceSurfaceKind, EdgeCurveKind  (top-level: below value) α₁
  geometry/mod.rs         facade: `mod x; pub use x::*;` + `pub use crate::geometry_id::*;`  α₂
  geometry/handle.rs      GeometryHandle, BRepKind, ReprKind                                   α₃
  geometry/capability.rs  Operation, CapabilityDescriptor{supports, queries, exports, projections}  α₃, ι₆
  geometry/op.rs          GeometryOp (Length/Angle-typed fields), descriptor table               α₃, θ
  geometry/query.rs       GeometryQuery (+reply_kind, reply_dimension, primary_handle), QueryReply  α₃, η, δ₃
  geometry/mesh.rs        Mesh, ValidatedMesh{mesh, tol, weld, waiver}, MeshContract*              α₂, ζ₁
  geometry/export.rs      ExportFormat, ExportOptions, STL/3MF writers                            α₂
  geometry/volume_mesh.rs ElementOrderTag (+polynomial_degree, nodes_per_tet), VolumeMesh          α₂, ζ₃
  geometry/error.rs       GeometryError (+from_mesh_violation), ExportError, TessError, QueryError  α₂
  geometry/naming.rs      TopologyAttribute*, *OpHistoryRecords, BooleanOpParents                  α₃
  geometry/kernel.rs      GeometryKernel (5 core + required accessors), the capability sub-traits,
                          KernelCapability, no_kernel_capabilities!/forward_kernel_capabilities!,
                          KernelRegistration + inventory::collect!                                 α₃, ε
  kernel_calls.rs         facade: one free fn per capability call, one absent-capability error each  ε₃–ε₅
  quantity.rs             Length, Angle (private f64; accept / from_metres / metres)                θ₁
  value/mod.rs + sampled_field.rs, selector.rs, display_number.rs, eval_status.rs                 α₄
crates/reify-eval/src/
  kernel_set.rs           KernelSet{entries: BTreeMap<KernelId, KernelEntry>, primary}             ι₃
  dispatcher.rs           select(&KernelSet, KernelRequest) -> Result<Selection, SelectionError>    ι₄–ι₆
  engine_build/mesh_handoff.rs   admit_mesh: the one ValidatedMesh front door                      ζ₂
  geometry_ops/query.rs   reply_to_value (the one QueryReply → Value point)                       η
crates/reify-test-support/src/
  kernel_assertions/{core, brep, modes, self_tests}.rs   assert_kernel_contract! (core; strict_taxonomy; real; unsupported)  δ₁
  kernel_doubles.rs       AllErrorKernel, ObservingKernel<K>                                      δ₆
```

### 4.1 Total delegation (ε; R1, L1–L3, L5)

GeometryKernel at main (rule: brace-matched parse of `pub trait GeometryKernel`) has 27 methods: 4 required and 23 defaulted, and a real kernel overrides every defaulted one. After #8284 there are 28; the 28th, `tessellate_with_topology`, is defaulted too (premise correction 11). Target:
- **Core (required, no defaults):** `execute`, `query`, `export`, `tessellate`, `reset`.
- **Capability sub-traits:** grouped by which kernels override them together and by receiver kind. An accessor must never force `&mut` onto a read path that holds only `&`; the voxel group is split into two traits for that reason.

| sub-trait | methods | accessor | implemented by (real) |
|---|---|---|---|
| `TopologyExtraction` | extract_edges, extract_faces, extract_vertices | `&mut self -> Option<&mut dyn _>` | OCCT, Manifold (writes an honest `extract_vertices` Err) |
| `MultiBody` | execute_split, make_compound | `&mut` | OCCT |
| `OpHistory` | execute_with_history | `&mut` | OCCT |
| `MeshIngest` | ingest_mesh, register_mesh_handle | `&mut` | Manifold, OpenVDB |
| `Voxelization` | ingest_mesh_at_resolution, densify_grid_to_sampled | `&mut` | OpenVDB |
| `VoxelSurfacing` | realize_mesh_from_voxel, surface_options_content_hash | `&self -> Option<&dyn _>` | OpenVDB |
| `VolumeMeshing` | mesh_surface_to_volume(_attributed), store_volume_mesh, volume_mesh | `&self` | Gmsh (deleted whole by meshing-service ε #8288) |
| `BRepProbe` | vertex_point, measure_mesh_deviation (closest_point_on_shape until ε₅, L3) | `&self` | OCCT |
| `KernelAttributeHook` (exists) | propagate_attributes | `&self` (unchanged) | Manifold |
| `BatchQuery` | query_many | `&self` | OCCT |
| `OptionedExport` | export_with_options | `&self` | OCCT, Manifold |
| `TopologyTessellation` | tessellate_with_topology (#8284) | `&self` | OCCT |

- That is twelve accessors: eleven new sub-traits plus the existing `attribute_hook`.
- Sub-trait methods carry no defaults either: a recording proxy that re-implements a sub-trait must not be able to mask it.
- `OcctKernelHandle` implements each sub-trait on itself with one-line forwards to its inherent methods; after S11 θ those methods are one-line `call(..)`s. Its accessors return `Some(self)`.
- `SingleKernelHolder` forwards each accessor in one line, `self.kernel.as_deref_mut()?.topology_extraction()`. Its 23 reproduced-default `None` arms (task/8533) go.
- Production call sites that move (rule: `.method(` through a `GeometryKernel` receiver, receivers read by hand; measured before #8284): 98, of which 89 are in reify-eval, 3 in reify-mesh-morph and 6 in reify-test-support. They route through `kernel_calls` (L2), so they change by path, not shape.
- **Same bug class in test code** (new evidence). `crates/reify-eval/tests/rounded_primitives_e2e.rs::RecordingKernel`, `crates/reify-eval/tests/harness_cache/unified_dag_geometry_executors.rs::OcctOpRecorder` and `crates/reify-eval/tests/fillet_curated_edges_e2e.rs::RecordingKernel` each document "forwarding all calls" but override 16 of 27 methods. They mask OCCT's `reset`, `closest_point_on_shape` and `vertex_point`. Under L1 they cannot compile without forwarding.

### 4.2 Contract suite and test doubles (δ; L5, L6)

`assert_kernel_contract!` keeps one macro entry with three orthogonal dimensions (h3):
- the **core**: Send + Sync and upcast; a dangling handle is never accepted on execute/query/export/tessellate; `query_many` length and single-vs-batch agreement; `extract_*` idempotence. It takes a `fixture` (how to mint a live handle: Box for OCCT/Fidget/Mock, `ingest_mesh(unit cube)` for Manifold/OpenVDB, none for gmsh) and a `probe` query kind the kernel answers (`SurfaceArea` for Manifold);
- a **taxonomy mode**: `strict_taxonomy` or `unsupported(substr)`;
- the **B-rep extension**: `extract_*` succeeds.

`real;` is re-expressed as core + strict + brep, and its generated test names are unchanged. The neutral core needs no behaviour change in any real kernel (predicted from the code, not run). Only MockGeometryKernel would fail it, because it accepts any dangling id; δ₄ adds an opt-in `strict_handles()`. Of the 52 bespoke doubles outside the kernel crates, reify-ir and test-support, about 35–40 are replaceable by `AllErrorKernel`, a Mock configuration or `ObservingKernel<K>`. The replaceable rule is stated in δ₇.

### 4.3 Mesh witness (ζ; R3)

```rust
pub struct ValidatedMesh { mesh: Mesh, tol: f64, weld: (Vec<[f32;3]>, WeldRemap), waiver: Option<MeshContractViolation> }
impl ValidatedMesh { pub fn mesh(&self) -> &Mesh; pub fn into_inner(self) -> Mesh; pub fn tol(&self) -> f64;
                     pub fn welded(&self) -> (&[[f32;3]], &WeldRemap); pub fn weldedness(&self) -> WeldednessReport;
                     pub fn waiver(&self) -> Option<&MeshContractViolation>; }
impl Mesh { pub fn check_mesh_contract(&self, tol: f64) -> Result<(), MeshContractViolation>;          // borrow: assertions, preflight
            pub fn into_validated(self, tol: f64, posture: MeshContractMode)
                -> Result<ValidatedMesh, Box<(Mesh, MeshContractViolation)>>; }                         // the only mint
```

- `validate` (it clones, and both production callers discard the witness) and `check_mesh_contract_welded` are deleted.
- `check_contract` becomes a five-line sequence of obligation functions: `check_buffer_shape`, `check_finite`, `check_index_bounds`, `check_non_degenerate`, `check_closed_and_consistently_wound` (the last two share one directed-edge map).
- `posture` is a required argument; tests pass `Enforce`.
- One reify-eval front door, `admit_mesh(mesh, tol, posture, source_kernel)`, feeds the existing `conversion_error`/Warn-diagnostic paths unchanged. It is called at the conversion handoff, the VolumeMesh edge, the Voxel edge and `realize_solid_sdf`.
- Kernel crates never mint. Manifold reads `vm.welded()`, so its double weld, its OOB-checked remap, `MANIFOLD_INGEST_TOL` and `static MODE` go. OpenVDB's hand-rolled length checks shrink to the inherent path.
- `MeshContractMode::from_env()` keeps one caller (`engine_admin.rs`).

### 4.4 Typed replies (η; L7)

```rust
pub enum QueryReply { Real(f64), Bool(bool), Point([f64;3]), Direction([f64;3]),
    BoundingBox{min:[f64;3], max:[f64;3]}, Matrix3([[f64;3];3]), PrincipalCurvatures{max:f64, min:f64},
    Indices(Vec<usize>), Handle(GeometryHandleId), SurfaceKind(FaceSurfaceKind), CurveKind(EdgeCurveKind),
    Datum(AnalyticDatum), /* transitional, η₁..η₅ only: */ Untyped(Value) }
impl GeometryQuery { pub fn reply_kind(&self) -> QueryReplyKind; pub fn reply_dimension(&self) -> Option<DimensionVector>; }
fn query(&self, q: &GeometryQuery) -> Result<QueryReply, QueryError>;   // core; query_many via BatchQuery
```

- `reply_kind()` is an exhaustive match with no wildcard, the `kind_name` precedent. It replaces 32 doc lines of reply-shape prose.
- `ShapeLocalTolerance`'s doc says `Real` while the code returns `Scalar`; that drift becomes unrepresentable.
- `QueryReply` accessors (`point(q)`, `bbox(q)`, `real(q)`, `as_f64()`, …) produce one uniform mismatch error. They replace the 5 tokenizers, 8 wrappers, 2 kind decoders and the "returned non-Bool/non-numeric" fallback arms.
- `FaceSurfaceKind`/`EdgeCurveKind` derive `EnumString`, `IntoStaticStr`, `VariantNames` and `EnumIter`. `try_from_str` goes, and the `expected one of Plane/…` literals are derived from `VARIANTS`, byte-identical.

### 4.5 Typed SI quantities (θ; R2)

```rust
#[repr(transparent)] pub struct Length(f64);  // finite SI metres from a LENGTH-dimensioned source
#[repr(transparent)] pub struct Angle(f64);   // finite SI radians from an ANGLE-dimensioned source
pub enum QuantityAcceptance<Q> { Accepted(Q), NonFinite(f64), Undefined, Rejected(ArgRejection) }
impl Length { pub fn accept(v: &Value) -> QuantityAcceptance<Length>; pub fn from_metres(m: f64) -> Option<Length>; pub fn metres(self) -> f64; }
```

- reify-eval's `accept_length_value`/`accept_angle_value` become thin diagnostic adapters over `accept`. The `required_length_*` family keeps its wording, Undef handling and group policy.
- Kernels read `.metres()`/`.radians()`, which names the unit at every read site (h1).
- Directions stay `[f64;3]`, and HalfSpace's normal becomes `f64` read through the direction reader.
- Once the 47 `Value` fields are typed, the warn-only tripwire, both extractors, the unused macro and the second classifier family are dead and are **deleted, not unified** (fk-08aa3fff3378 closes by removal).

### 4.6 Kernel selection (ι; R1, L8)

Production kernel-map lookups at main number **42** (rule: a multi-line-aware scan of `(geometry_kernels|kernels).get(_mut)?(` in production regions of crates/ and gui/src-tauri). By key:
- 23 key on `default_kernel_name`;
- 4 follow the dispatcher plan, one of them with a default fallback;
- 7 follow the handle's own kernel with a silent default fallback;
- 7 are hard-coded: Gmsh 2, OpenVdb 5.

After #6308 there are 37.

```rust
pub struct KernelSet { entries: BTreeMap<KernelId, KernelEntry>, primary: Option<KernelId> }   // field of Engine
enum Declared { Descriptor(CapabilityDescriptor), Injected }
pub enum KernelRequest<'a> { Op{op, demanded, available, prefer: Option<KernelId>}, OnHandle{owner: KernelId, need: HandleNeed},
                             Convert{from: ReprKind, to: ReprKind}, Project{from: ReprKind, to: ProjectionKind} }
pub fn select(set: &KernelSet, req: KernelRequest<'_>) -> Result<Selection, SelectionError>;  // Selection: private fields
pub enum SelectionError { NoKernelChain{..}, OwnerAbsent{owner, need}, NoProvider{..}, MixedOwners{owners} }
// codes: OwnerAbsent(query) → QueryKernelUnserved; owner mismatch → QueryKernelMismatch (#8391);
//        NoKernelChain / NoProvider (ops, converts, projects, make_compound) → NoKernelChain; MixedOwners → #6308's code
```

- `select` reads only the set's own descriptors, never `collect_registry()`. That is what fixes the boolean STL.
- An absent owner is an Error diagnostic, never a different kernel. A handle id is meaningless in another kernel's id space, as #6308's `kernel_owner.rs` doc says, and `kernel_owner.rs` is absorbed into `select`.
- `Engine::DEFAULT_KERNEL_NAME`, `kernel_id_for_registry_name` (whose own revisit trigger has fired), the three-tier fallback in `realization_content` and the `repr_capable_kernel` field go.
- **Constructor signatures are unchanged.** `Engine::default_kernel_name()` keeps its signature and returns `"occt"` instead of the sentinel.
- The selector is a free function over the field, not an `&self` method on Engine, because call sites need disjoint field borrows (engine_admin.rs's own note).

## 5. Contracts and boundary tests (G5: multi-kernel is load-bearing → B + H)

**C1 — Total delegation (ε₂, INV-GEO-5).**
- At ε₂: no core method or accessor has a default body, so every accessor is required. At ε₅ (the end state): `trait GeometryKernel` has zero items with a default body.
- `forward_kernel_capabilities!` forwards every accessor; a unit test builds a wrapper with it over a full-capability kernel and asserts `Some` for every `KernelCapability::iter()`.
- A delegating impl (one forwarding a required core method to a field) either invokes `forward_kernel_capabilities!(<that field>)` or forwards every accessor to that same field. A masked accessor needs an annotated `// INV-GEO-5 masked: <reason>`, counted with a ratchet at 0 (L1).

| boundary test | side | precondition | postcondition |
|---|---|---|---|
| BT-ε1 | producer | `crates/reify-geometry/tests/forwards_every_kernel_method.rs` (from #8533): its syn oracle reads the trait from its new file | every core method and accessor is reached through the holder; the empty holder returns `None` for every accessor |
| BT-ε2 | lint | the delegation lint (`crates/reify-geometry/tests/kernel_wrappers_forward_every_capability.rs`) walks `crates/*/src` and `crates/*/tests` at run time; its seeded violations are inline source strings, not files, so it never reds on itself | green on the workspace. Red on each seeded case: a wrapper that forwards `execute` and uses `no_kernel_capabilities!`; a delegating wrapper whose hand-written accessor returns `None`; a trait item with a default body |
| BT-ε3 | consumer | `crates/reify-eval/tests/holder_forwards_real_occt.rs` (OCCT, reify-eval dev-deps): an e2e through `SingleKernelHolder` over real OCCT calling `vertex_point` and `reset` | real coordinates and a cleared store, never "not supported" or a silent no-op |

**C2 — Contract core on every kernel (δ, INV-GEO-4).** Every real kernel and MockGeometryKernel (strict) instantiate the core, and strict taxonomy holds where a path is supported (L6).

| boundary test | side | postcondition |
|---|---|---|
| BT-δ1 | per kernel | `crates/reify-kernel-{fidget,manifold,openvdb,gmsh}/tests/kernel_contract.rs` and the OCCT `real;` arm green |
| BT-δ2 | mock | `crates/reify-test-support/tests/mock_kernel_contract.rs` green on `MockGeometryKernel::new().strict_handles()` |
| BT-δ3 | negative self-tests | a kernel accepting a dangling export, and one returning `FormatError` where `ExportError::InvalidHandle` is required, are each caught |

**C3 — Mesh witness (ζ₂, INV-GEO-1).** The five mesh-consuming capability methods take `&ValidatedMesh`. The only mint is `Mesh::into_validated(tol, posture)`. In production it is called only at reify-eval's `admit_mesh`; tests mint through `reify_test_support::validated` and reify-ir's own tests. Finite and IndexValid hold for every witness.

| boundary test | side | postcondition |
|---|---|---|
| BT-ζ1 | producer (reify-ir) | `into_validated_under_warn_waives_semantic_but_not_index_valid`; `witness_weld_equals_weld_positions` |
| BT-ζ2 | consumer (engine) | `crates/reify-eval/tests/mesh_contract_handoff_diagnostic.rs` Warn/Enforce semantics unchanged; `crates/reify-eval/tests/manifold_cross_kernel_real.rs::engine_routes_overlapping_box_union_to_manifold_mesh` (the real tessellate → Manifold ingest path under `with_registered_kernels`) green, and Manifold no longer re-validates (`git grep -n check_mesh_contract_welded -- crates` = 0); `reify build examples/multi_kernel/voxel_to_mesh.ri -o x.stl` (tessellate → OpenVDB `ingest_mesh`) has the same `Triangles:` as before the leaf |
| BT-ζ3 | adoption | ratchet row `mesh-mint-outside-front-door` at its post-leaf count |

**C4 — Typed replies (η, contract core extension).** For every query a kernel answers, `reply.kind() == query.reply_kind()`. Every eval consumer reads through `QueryReply` accessors. The one Value conversion is `reply_to_value`.

| boundary test | side | postcondition |
|---|---|---|
| BT-η1 | producer | contract core: reply-kind agreement on OCCT and Manifold for each answered variant |
| BT-η2 | consumer, user surface | `reify eval tests/prd-gate/fixtures/kernel_interface_query_reply_oracle.ri`: stdout and stderr captured at the leaf's base commit in the same lane, and byte-identical after it; S13's oracles O1–O3 unchanged |
| BT-η3 | deletion | ratchet rows (Untyped, JSON literals, parser fns) at 0 after η₆ |

**C5 — Typed quantities (θ).** No `GeometryOp` field is `Value`. Every quantity field is `Length`/`Angle` (or `[Length;3]`, `Vec<[Length;3]>`). Every bare numeric field carries a justification row.

| boundary test | side | postcondition |
|---|---|---|
| BT-θ1 | type | `crates/reify-ir/tests/geometry_op_quantity_fields.rs`: `Value`-field count at baseline (47 → 0 across θ₁–θ₃); the justification guard red on an unjustified new `f64` field |
| BT-θ2 | user surface | the half-space fixture names `half_space` and `nz`; `examples_smoke` green (no corpus file changes meaning) |

**C6 — One selector (ι, INV-GEO-2 consumer clause).** `select` returns only a kernel the set holds. A handle-served request goes to the handle's owner or fails with `OwnerAbsent`.

| boundary test | side | postcondition |
|---|---|---|
| BT-ι1 | producer | dispatcher unit test: no `Selection` names a kernel absent from the set |
| BT-ι2 | consumer, user surface | `reify build tests/prd-gate/fixtures/kernel_interface_boolean_stl_export.ri -o x.stl` exits 0 with no `internal error`, and the STL spans x in [-5, 10] mm |
| BT-ι3 | consumer | `crates/reify-eval/tests/manifold_cross_kernel_real.rs` extended (#6137's test, L14): under `with_registered_kernels` the Manifold-owned overlapping-box union is exported **by Manifold**, and the STL spans x in [-5, 10] mm, not one operand. `SurfaceArea`/`BoundingBox` on that handle are answered by Manifold. A Mesh-only owner asked for `make_compound` yields a coded Error, not an OCCT compound. Implicit-solids γ #8391's B14 (`implicit_solids_owner_routing_isosurface.ri`) stays green through ι₅ |
| BT-ι4 | no regression | `reify build examples/affine_tapered_spacer.ri -o x.step` rc 0 (needs ι₂); `reify build examples/multi_kernel/voxel_to_mesh.ri --verbose` producer lines unchanged and `Triangles:` ≥ 10000 (inherited from #6308) |

**C7 — Native deps fail closed (β₁).** A missing OCCT, gmsh or OpenVDB is a build error naming the dependency and `scripts/setup-dev.sh`, from every build script that detects one: reify-kernel-occt, reify-kernel-gmsh, reify-kernel-openvdb, reify-kernel-conformance, reify-cli, reify-eval, reify-mesh-morph and gui/src-tauri (Fidget and Manifold detect nothing). No `cfg(not(has_*))` code exists. The silent-vacuity rule of `scripts/check-manifold-deps.sh` holds by construction, and the preflight stays as the fast first failure.

## 6. Pre-conditions and coordination with existing tasks

Read with `get_task`/`search_tasks`, 2026-10-13. Decomposition re-runs contract §8 dedup.

- **In flight; every leaf whose change depends on their content depends on them** (a leaf that merely shares a file rebases):
  - #8533 (holder forwards plus syn guard; the guard `include_str!`s geometry.rs);
  - #8529 (manifold catch-all messages);
  - #8652 (S11 γ1: `op_args.rs`, the history lowering);
  - **#6308**, which landed in `cf59570562` (export and terminal-owner routing; adds `kernel_owner.rs`; the 2-triangle STL fix). The edges on it are kept as already satisfied.
- **Upstream program leaves:**
  - S11 θ #8672 (`call<R>`; R1 orders ε₁ and η₁ after it);
  - S11 ι #8684 (OCCT `query.rs`; η₁);
  - S11 ζ #8669 (`ReprPolicy` rewrites every OCCT execute arm; θ₁);
  - S13 β #8748 (`geometry_ops/args.rs`; η₄);
  - S13 ε #8751 (the end of S13's chain, `geometry_ops/tests.rs` deleted; η₁);
  - S15 δ #8686 (`registry_free`; α₄ and α₅);
  - S15 μ #8711 and ξ #8717, S3 ι #8704, S6 η #8834 (they edit value.rs and pin delivered checks to it; α₄).
- **Other pending tasks:**
  - #7867 (remove `type_id`; α₅ after it);
  - #7636 (gmsh `tet_readback.rs` relocation; ζ₃ after it);
  - #6104 (boolean STL; its prescribed `with_registered_kernels` swap composes with ι₄; **#6104 depends on ι₄**, L15);
  - **implicit-solids γ #8391** (owner-routed queries, QueryKernelMismatch/QueryKernelUnserved; ι₅ depends on it, L14);
  - #6366 (split by L14: ι₅ takes `distance_between_placed`; #6366 keeps the declarative `: Output` export and `resolve_export_body_color` and **depends on ι₅**);
  - #8284 (meshing-service α: `tessellate_with_topology`; α₂ and ε₁ come after it).
- **Subsumed at decomposition** (cancelled with a reason naming the leaf, after porting their content):
  - **#8532** → β₁ (the openvdb crate doc becomes one-arm);
  - **#8790** → α₄ (value.rs measure and partition);
  - **#8795** → θ₃ (`define_kernel_length_extract!` and the stale construction-site note in kernel_validation.rs);
  - **#6137** → ι₅ (union export under `with_registered_kernels`; the Mesh-only-owner `make_compound` decision; its ExportBody half is #6308's);
  - **S14 κ #8632** → β₁ (L13; its key fk-c497935d4b8a moves onto β₁; dependents rewired);
  - **#6474** → γ (the B7+ layer pairs: γ's stratum table plus its expr/compiler crate rule; its content is ported).
- **Stamped** (R4): fk-92d313d2986f onto **#8285** (`metadata.x_finding_key`, merged as a list). A details note names the ratchet rows the session ships: lock, size scope, capture and terminal sites = 1 each; `"Mesh.ElementOrder"` writes = 1; trailing clear on every exit path.
- **Referenced, not re-filed:** #8533 (fk-ef5f5da8f729), #8529 (fk-10ec3d2a1b76), #8561 (fk-65bb1ffb5d60; γ lists the occt → mesh-morph optional edge as an exception tagged #8561, and a note asks #8561 to lower that row in its own diff).
- **Delivered-check widening before filing (L9):** #8284's `fn tessellate_with_topology\b` (pinned to `crates/reify-ir/src/geometry.rs`) and #8686/#8711/#8717's checks pinned to `crates/reify-ir/src/value.rs` are widened to `crates/reify-ir/src`, in the three writes. This is done by the decompose session, never by a leaf.
- **meshing-service interplay** (notes appended to the task records at decomposition; no edges beyond those listed):
  - #8284 α's `tessellate_with_topology` lands as a defaulted method before ε₁, which moves it into `TopologyTessellation` (premise correction 11).
  - #8285 β's `GeometrySource::Surface(Arc<Mesh>)` should carry a `ValidatedMesh` once ζ₂ lands, and the worker re-mints across bincode.
  - #8288 ε deletes the `VolumeMeshing` sub-trait and accessor, not four flat methods, if ε₄ landed first.
  - β #8285 / γ #8286 find no gmsh stub arms after β₁.

## 7. Cross-PRD relationship and seam owners (G4)

| shared artefact | owner | S10's role |
|---|---|---|
| GeometryKernel trait (27 methods at main, 28 after #8284; 79 implementors incl. MockGeometryKernel) | **S10** | ε reshapes it (C1) |
| reify-test-support `assert_kernel_contract!` and MockGeometryKernel | **S10** | δ (C2) |
| reify-kernel-conformance producer × consumer matrix; INV-GEO-1..4 rows | **S10** | rows edited by ζ₂ (GEO-1), ι₅ (GEO-2), δ₄ (GEO-4); GEO-3 is **S11 ε #8682**'s |
| OcctKernelHandle inherent surface | S11 | **R1:** S10 edits only trait/sub-trait impl blocks and signature-following hunks in handle.rs, after #8672 |
| `KernelSet` type (new) | **S10** (R1) | ι₃ defines it; S1's constructor consumes it |
| build-routing reads of the global kernel registry (part of S1's fk-4db43199eda2) | **S10** (R1) | ι₄ deletes them; S1 keeps link anchors, its constructor and `test_registry_override` |
| Engine constructor signature | S1 | **unchanged** by every S10 leaf (L8) |
| `reify_ir::arg_acceptance` | S15 | consumed unchanged by θ (R2) |
| `GeometryError::BackendUnavailable` (S14 κ #8632) | S14 | its only producer is the gmsh stub arm, which R5 removes; **β₁ subsumes κ** (L13) |
| owner-routed-or-rejected queries, `QueryKernelMismatch`/`QueryKernelUnserved` | implicit-solids γ **#8391** | ι₅ consumes them and re-expresses the owner check through `select` (L14) |
| `distance_between_placed` | **S10 ι₅** (L14) | was #6366 part (1) |
| declarative `: Output` export, `resolve_export_body_color` | **#6366** | #6366 depends on ι₅ and builds on `select(OnHandle)` once `Value::GeometryHandle` carries its kernel |
| `crates/reify-cli/tests/harness_cli/cli_build_boolean_stl.rs` | **S10 ι₄** (L15) | #6104 depends on ι₄ and extends it |
| meshing-service gmsh sequence (`run_mesh`, the option scope) | meshing-service β #8285 | carries fk-92d313d2986f (R4) |
| `reify_core::Diagnostic` shape | S12 | unchanged. ι mints no new code: an absent owner on a query maps to `QueryKernelUnserved`, an owner mismatch to `QueryKernelMismatch` (both #8391's), and an op, convert or project request with no provider (including `make_compound` on a Mesh-only owner) to `NoKernelChain`. `MixedOwners` reuses the code #6308 lands for mixed export owners. θ₅ uses `DimensionedArgRejected` |
| `docs/invariants.md` row format and pointer grammar | S24 | content edits here; pointers inline before #8821, in the Pointer column after |
| `scripts/module-scc-baseline.json` reify-ir row | S3 tool | α₁ lowers it in its own diff |
| reify-ir `lib.rs` re-export block | (lock-level only) | S6 β/γ add modules there; no ordering edge |

## 8. Decomposition plan

```
reify-ir      α₁ ─► α₂ ─► α₃ ─────────────────────────────┬──────────┬───────────┐
              α₁ ─► α₄ ─► α₅            (#8533, #8284, #7379 ─► α₂)│  │           │
stubs         β₁ ─► β₂ ─► β₃ ; β₁ ─► δ₁                │          │           │
strata        γ                                            │          │           │
contract      δ₁ ─► δ₂ ─► δ₃ ◄─ α₃ ; δ₃ ─► δ₄ ;  δ₁ ─► δ₆ ─► δ₇ ◄─ δ₄            │           │
delegation    (α₃, #8533, #8672 ◄ #8284, β₁) ─► ε₁ ─► ε₂ ◄─ δ₇ ; ε₂ ─► ε₃ ─► ε₄ ─► ε₅ ◄─ η₂ ; ε₂ ─► η₁
mesh          α₂ ─► ζ₁ ─► ζ₂ ◄─ ε₄, #6308 ; ζ₂ ─► ζ₄ ;  (#7636, α₂) ─► ζ₃
replies       (α₃, ε₂, δ₃, δ₇, #8533, #8684, #8751, #8672) ─► η₁ ─► η₂ ─► η₃ ; η₁ ─► η₄ ◄─ #8748 ; η₁ ─► η₅ ; η₂..η₅ ─► η₆
quantities    θ₅ ─► θ₁ ◄─ (α₃, #8652, #8669) ; θ₁ ─► θ₂ ─► θ₃ ─► θ₄
selection     ι₁ (independent) ; ι₂ ─► ι₃ ◄─ #6308 ; ι₃ ─► ι₄ ─► ι₅ ◄─ #8391 ; ι₅ ─► ι₆ ◄─ ε₄, ι₂ ; ι₄ ─► #6104 ; ι₅ ─► #6366
close         ω ◄─ every leaf
```

Priorities follow the severity of the stamped key, and enablers take medium. **High:** ε₅ #8969 (fk-047cb467bd07) and η₃ #8976 (fk-59c46755d24c); both are dependencies of the trigger-chain gate #8612. **Low:** ι₁ #8987. Everything else is medium. Every leaf: `task_kind = "normal"`. Before editing, run `git diff 274a212e0a..main -- <files>` and re-read the symbols; `python3 scripts/assert-module-scc.py check` stays OK. **Same-diff obligations for file-moving leaves (α₂, α₃, α₄, δ₁, β₁):** after S24 ο #8822 lands, run `plinecite-baseline-gen --rename <old>=<new>` for every moved file under PLINECITE scope; after #8821 lands, update any docs/invariants.md Pointer naming a moved symbol.

### α₁ (#8930) — geometry_id.rs breaks the value↔geometry SCC
- **Keys:** fk-66c1d2fe9ff9. **INV:** INV-ARCH-1 (row lowered).
- **Files:** `crates/reify-ir/src/geometry_id.rs` (new), `crates/reify-ir/src/geometry.rs`, `crates/reify-ir/src/value.rs`, `crates/reify-ir/src/boundary_attachment.rs`, `crates/reify-ir/src/lib.rs`, `scripts/module-scc-baseline.json`.
- **Deps:** none.
- **Work:** move GeometryHandleId, KernelHandle (with the KernelId re-export), the FeatureId family (DerivedKind, FeatureIdParseError and impls), AxisSign, CapKind, ModEntry, Role (with `pub(crate) content_hash_bytes`), FaceSurfaceKind and EdgeCurveKind into a top-level `geometry_id.rs` importing only `reify_core`. `geometry.rs` re-exports them (`pub use crate::geometry_id::{…}`), so every path and its test module resolve. Repoint value.rs and boundary_attachment.rs. Lower the baseline in the same diff.
- **Signal:** `python3 scripts/assert-module-scc.py check` is OK with the reify-ir row `[[completeness, ranked], [expr, value]]`. It goes red (GREW) if `use crate::geometry::Role` is re-added to value.rs. `git grep -n 'crate::geometry' -- crates/reify-ir/src/value.rs crates/reify-ir/src/boundary_attachment.rs` returns 0 production matches.

### α₂ (#8931) — geometry.rs → geometry/ directory module, lower strata
- **Keys:** none (h14 half of fk-66c1d2fe9ff9).
- **Files:** `crates/reify-ir/src/geometry/mod.rs` (git mv of geometry.rs), `geometry/mesh.rs`, `geometry/error.rs`, `geometry/export.rs`, `geometry/volume_mesh.rs` and their test children; `crates/reify-ir/tests/cube_fixture_agreement.rs` (`LOCAL_PATH`); `crates/reify-audit/tests/new_orphans_2026_06_09_g_allow.rs` (`write_stl_ascii` pin); `crates/reify-geometry/tests/forwards_every_kernel_method.rs` (the `include_str!` path); `crates/reify-eval/src/engine_build.rs`, `crates/reify-kernel-gmsh/src/mesh_boundary.rs`, `crates/reify-kernel-manifold/src/kernel.rs` (`into_geometry_error` callers).
- **Deps:** α₁, #8533, #8284, #7379 (each edits geometry.rs; L9).
- **Work:** this is a move, with one real change: `GeometryError::from_mesh_violation(kernel, v)` in error.rs replaces `MeshContractViolation::into_geometry_error`, which closes the mesh↔error cycle (premise correction 10). The facade keeps every `reify_ir::geometry::X` path; 11 names used through that path are not re-exported at the crate root. Shared test fixtures go in `geometry/test_fixtures.rs`. Stale comments citing `reify_ir::geometry::tests::…` are fixed in the comments. No task-record writes (L9).
- **Signal:** `git grep -c into_geometry_error -- crates` = 0. Each new file's production region is under 1,500 lines. `cube_fixture_agreement` and the audit pin are green. `cargo test -p reify-ir` passes with the same test count, counted once per `scripts/cargo-test-tally.py`.

### α₃ (#8932) — geometry/ upper strata
- **Keys:** none.
- **Files:** `crates/reify-ir/src/geometry/{handle,capability,op,query,naming,kernel}.rs` and test children; `crates/reify-geometry/tests/forwards_every_kernel_method.rs` (syn oracle path → `geometry/kernel.rs`).
- **Deps:** α₂.
- **Work:** as in §4. `KernelRegistration` and `inventory::collect!` live in kernel.rs, because placing them in capability.rs would make capability → kernel → op → capability a cycle.
- **Signal:** `ls crates/reify-ir/src/geometry/` lists the 10 children plus `mod.rs`. No production region exceeds 1,500 lines (op.rs's descriptor table is data). #8533's guard still enumerates the trait from its new file.

### α₄ (#8933) — value.rs → value/ directory module
- **Keys:** none (the record's value.rs h14 partition). **Subsumes #8790.**
- **Files:** `crates/reify-ir/src/value/mod.rs` (git mv), `value/{sampled_field,selector,display_number,eval_status}.rs` with test children; `crates/reify-ir/src/lib.rs`; `crates/reify-audit/tests/new_orphans_2026_05_16_g_allow.rs`.
- **Deps:** α₁, #8686, #8711, #8717, #8704, #8834.
- **Work:**
  - `GeometryHandleRef::from_geometry_handle(&Value)` becomes `Value::as_geometry_handle_ref(&self)`, at 23 sites.
  - `ValueMap` and the three Value-taking display helpers stay in the core (type recursion).
  - Carving out `value/compare.rs` and `value/display.rs` is optional, decided by measurement.
  - Port #8790's content first.
  - The decompose session has already widened the delivered checks that pin `value.rs` to `crates/reify-ir/src` (L9), so this leaf writes no task record.
- **Signal:** `reify-audit --pattern PDCHECK` is clean. Every `value/*.rs` production region is under 2,000 lines. The SCC check is OK, with no new top-level node.

### α₅ (#8934) — Typed StructureInstanceData overlays
- **Keys:** fk-7b093573458b.
- **Files:** `crates/reify-ir/src/value/mod.rs`, `crates/reify-ir/src/lib.rs`, `crates/reify-ir/tests/api_surface.rs`, `crates/reify-eval/src/engine_eval.rs`, `crates/reify-eval/src/compute_targets/elastic_static.rs`, `crates/reify-expr/src/lib.rs`, `crates/reify-compiler/tests/harness_mechanics/flexures_stdlib_compile.rs`, `crates/reify-compiler/tests/harness_structure_declarations/same_module_structure_ctor_compile.rs`, `tests/infra/adoption-ratchet.manifest`; plus the literal sites (rule: `git grep -n 'StructureInstanceData {' -- crates gui`, construction-shaped).
- **Deps:** α₄, #8686, #7867.
- **Work:**
  - `annotations: MaterializedAnnotations` and `source_span: Option<SourceSpan>` become typed fields that the hand-written Eq/Ord/Hash/content-hash/Display never read, so exclusion holds by construction.
  - `user_fields()` and both `@@` constants go.
  - Step one moves the literal sites onto constructors (`registry_free` or a named constructor), so adding a field is not a 299-site edit.
- **Signal:**
  - `git grep -n '"@@' -- crates/reify-ir/src` = 0.
  - `git grep -n 'user_fields' -- crates/reify-ir/src` = 0.
  - A test asserts that two instances differing only in `source_span`/annotations are `==`, `Equal` and hash-equal.
  - The ported overlay tests pass.
- **Ratchet:** `structure-instance-literal-eval | ^[^/]*StructureInstanceData [{] | crates/reify-eval/src/**/*.rs | <post-step-one count> | α₅`, plus `-stdlib` and `-expr` rows over `crates/reify-stdlib/src/**/*.rs` and `crates/reify-expr/src/**/*.rs`, each measured at the leaf's base commit (L12).

### β₁ (#8935) — Native dependencies fail closed; the stub arms are deleted (R5)
- **Keys:** fk-107e28de9029, fk-0f933e75bf7d; plus S14's fk-c497935d4b8a from the subsumed κ. **Subsumes #8532 and S14 κ #8632** (L13). **INV:** INV-GEO-4 (the stub-arm clause is deleted from the row).
- **Files:**
  - Build scripts: `crates/reify-kernel-occt/build.rs`, `crates/reify-kernel-gmsh/build.rs`, `crates/reify-kernel-openvdb/build.rs`, `crates/reify-kernel-conformance/build.rs`, `crates/reify-cli/build.rs`, `crates/reify-eval/build.rs`, `crates/reify-mesh-morph/build.rs`, `gui/src-tauri/build.rs`; `crates/reify-build-utils/src/lib.rs` (`require`).
  - OCCT: `crates/reify-kernel-occt/src/stubs.rs` (deleted), `crates/reify-kernel-occt/src/lib.rs`.
  - gmsh: `crates/reify-kernel-gmsh/src/kernel.rs` (stub; deleted), `crates/reify-kernel-gmsh/src/lib.rs`, `crates/reify-kernel-gmsh/src/mesh_profile_2d.rs` and `crates/reify-kernel-gmsh/src/refine_volume.rs` (stub arms).
  - OpenVDB: `crates/reify-kernel-openvdb/src/kernel.rs` (stub; deleted), `crates/reify-kernel-openvdb/src/lib.rs` (one-arm crate doc).
  - solver-elastic and eval: `crates/reify-solver-elastic/src/mesher.rs`, `crates/reify-solver-elastic/src/volume_refine.rs` (the stub-only "not available" routing and the producer-less `Mesh2dError::GmshUnavailable` / `RefineError::GmshUnavailable` variants), `crates/reify-eval/src/compute_targets/elastic_static.rs` (its "not available in this build" refinement diagnostic), and the stub-mode assertions in `crates/reify-eval/src/engine_build/dispatch_volume_mesh_tests.rs` and `crates/reify-eval/src/sweep_classifier.rs`.
  - `scripts/check-manifold-deps.sh` (its "THE SILENT-VACUITY RULE" header describes the fail-open build that β₁ removes). It is a verify-pipeline file, so this leaf takes the full gate (`verify-pipeline-guard.sh requires-full-gate` exits 0).
  - `crates/reify-test-support/src/kernel_assertions.rs` (`assert_stub_kernel_errors!` and the `stub;` arm, if no user remains).
  - `CLAUDE.md` (Native deps bullet); `docs/invariants.md` (INV-GEO-4).
- **Deps:** none.
- **Work:**
  - Every build script listed calls `reify_build_utils::require(NativeDep::…)` instead of `find(..)` + `return`. `require` panics with a message naming the dependency, the `*_LIB_DIR`/`*_INCLUDE_DIR` overrides and `scripts/setup-dev.sh`.
  - Delete the three stub arms, their `#[cfg(all(test, not(has_*)))]` test modules, `STUB_UNAVAILABLE_MARKER`, the `map_geometry_error` substring routing in both solver-elastic copies, and every variant, diagnostic and test the stubs alone produced (listed under Files). After this nothing produces "not available in this build". κ #8632 is cancelled before β₁ is filed (L13).
  - Rewrite the check-manifold-deps.sh header for a build that fails closed, keeping the preflight as the fast first failure.
  - `has_*` stays emitted (always true) until β₂/β₃ sweep it.
  - Rewrite the openvdb crate doc for its single arm (absorbing #8532).
  - Update CLAUDE.md's Native-deps bullet: absence now fails the build, and check-manifold-deps.sh stays as the fast first failure.
- **Signal:**
  - The decision is unit-tested where it lives: `require`'s pure core, given an injected empty candidate list, produces a message naming the dependency, both overrides and `scripts/setup-dev.sh`.
  - `git grep -nE 'reify_build_utils::find\(' -- '*build.rs'` = 0, so every detector goes through `require`.
  - `git grep -n 'not(has_occt)\|not(has_gmsh)\|not(has_openvdb)' -- crates gui` = 0.
  - `git ls-files crates/reify-kernel-occt/src/stubs.rs` is empty.
  - The INV-GEO-4 row no longer names a stub arm.
  - Nobody can reach the absent path end to end on a provisioned host (the override is trusted and the system path is found); the commit message says so.

### β₂ (#8936) — Sweep always-true native cfgs: kernel and support crates
- **Keys:** none.
- **Files:** `[]` (a broad mechanical sweep; the footprint is the kernel crates, `crates/reify-geometry`, `crates/reify-test-support`, `crates/reify-kernel-conformance`, `crates/reify-build-utils`, `tests/infra/adoption-ratchet.manifest`).
- **Deps:** β₁.
- **Work:**
  - Delete the `cfg(has_*)`/`cfg_attr` gates, the `#![cfg(has_*)]` file gates and the `*_AVAILABLE` guards in these crates; their value is now always true. **The `OCCT_AVAILABLE` and `GMSH_AVAILABLE` constants stay**, set to `true`, because most of their readers are in β₃'s crates. β₃ deletes the constants.
  - Delete the `stub_register` features of reify-kernel-gmsh and reify-kernel-openvdb, their self-dev-dep activations and the `cfg(any(has_*, feature = "stub_register"))` gates on `inventory::submit!`.
  - Stop emitting a cfg from a build script once its crate has no reader of it.
- **Signal:**
  - The rows below are at 0 for these crates.
  - `git grep -n stub_register -- crates/reify-kernel-gmsh crates/reify-kernel-openvdb` = 0 (β₃ takes the comment mentions elsewhere).
  - Each crate's test count is not lower than before (an un-gated test that now runs is expected), counted once per crate with `scripts/cargo-test-tally.py`.
- **Ratchet:** `native-cfg-gate-<crate> | ^[^/]*has_(occt|gmsh|openvdb) | crates/<crate>/**/*.rs | 0 | β₂`, one row per crate in the footprint (L12: one id per glob). No guard row yet: the constants' definitions stay until β₃.

### β₃ (#8957) — Sweep always-true native cfgs: reify-eval, front ends, the rest; the constants go
- **Keys:** none.
- **Files:** `[]` (footprint `crates/reify-eval`, `crates/reify-cli`, `gui/src-tauri`, `crates/reify-mesh-morph`, `crates/reify-solver-elastic` and the remaining crates, plus the two constant definitions in `crates/reify-kernel-occt/src/lib.rs` and `crates/reify-kernel-gmsh/src/lib.rs`; `tests/infra/adoption-ratchet.manifest`).
- **Deps:** β₂.
- **Work:**
  - As β₂, over these crates (`OCCT_AVAILABLE` has readers in 110 reify-eval files, 15 reify-cli, 4 gui and 1 solver-elastic; `GMSH_AVAILABLE` in 2 reify-eval, 1 mesh-morph and 6 solver-elastic).
  - Once no reader remains, delete both constants.
  - The link anchors `extern crate … as _` / `use … as _` in `crates/reify-cli/src/main.rs` and `gui/src-tauri/src/lib.rs` keep their lines and lose only their `cfg` (S1 owns the anchors, §12).
  - `gui/src-tauri/src/kernel_status.rs::current_kernel_status` calls `kernel_status_for(true)`, so it returns "available" unconditionally. The `get_kernel_status` command and its TS consumer stay as they are (S25/S26, §12).
  - Remove the `stub_register` comment mentions in `crates/reify-eval/Cargo.toml`, `crates/reify-mesh-morph/Cargo.toml`, `crates/reify-cli/src/main.rs` and `crates/reify-eval/src/realize_solid_sdf.rs`.
  - meshing-service ζ #8289 also deletes `GMSH_AVAILABLE`; whichever lands second finds it gone.
- **Signal:**
  - `git grep -nE '^[^/]*(OCCT|GMSH|OPENVDB)_AVAILABLE|has_(occt|gmsh|openvdb)' -- crates gui` = 0.
  - `git grep -n stub_register -- crates gui` = 0.
  - Workspace test counts are measured as in β₂.
- **Ratchet:** `native-cfg-gate-<crate>` rows for these crates, at 0. `native-available-guard | ^[^/]*(OCCT|GMSH|OPENVDB)_AVAILABLE | crates/**/*.rs | 0 | β₃`.

### γ (#8958) — Crate strata assertion
- **Keys:** fk-448317051ff9. **Subsumes #6474** (assert-crate-dag.sh B7+ for the expr/compiler/eval/application pairs): the stratum table forbids language → engine and engine → frontend edges, and the crate rule below covers the expr/compiler pair. Dev-dependencies (for example reify-eval's deliberate dev-dep cycle with reify-runtime) are not counted.
- **Files:** `scripts/assert-crate-strata.py` (new, stdlib), `scripts/crate-strata.json` (new), `crates/reify-build-utils/tests/crate_dag_assertion.rs` (test functions added; no new binary), `.jcodemunch.jsonc` (pointer comment only).
- **Deps:** none.
- **Membership** (all 37 packages from `cargo metadata --no-deps` at 274a212e0a; an unclassified package is red):

  | stratum | packages |
  |---|---|
  | spine | core, ast, ir, syntax, tree-sitter-reify, builtins, build-utils, config, doc |
  | language | compiler, stdlib, expr, constraints, fdm, gcode, doc-build |
  | kernel | kernel-fidget, kernel-manifold, kernel-occt, kernel-openvdb, kernel-gmsh, geometry |
  | solver | solver-elastic, shell-extract, compute-contract |
  | analysis | (S5's new crate; classified in S5's own diff) |
  | engine | eval, runtime, mesh-morph |
  | frontend | cli, lsp, mcp, gui |
  | test-support | test-support, kernel-conformance, eval-fea-tests, spec-conformance |
  | tooling | audit |

- **Edge rule** (normal and build edges, optional ones included). Each stratum may depend on itself and on:

  | stratum | may depend on |
  |---|---|
  | spine | nothing else |
  | language | spine |
  | kernel | spine |
  | solver | spine |
  | analysis | spine, language, solver |
  | engine | everything above except test-support and tooling |
  | frontend | everything above except test-support and tooling |
  | test-support | everything except tooling |
  | tooling | everything |

- **Crate rules:**
  - a kernel crate depends only on {core, ir, build-utils};
  - reify-geometry depends only on {ir};
  - compute-contract depends only on spine (S5 ruling);
  - reify-expr and reify-compiler do not depend on each other (from #6474; true today);
  - no production stratum depends on test-support.
- **Exception list.** It must equal today's violations, and it fails in both directions (a new violation and a stale row). Derived from the graph above, it holds exactly three rows:
  - kernel-occt → mesh-morph (optional; #8561);
  - solver-elastic → kernel-gmsh (meshing-service ζ #8289 drops it);
  - compute-contract → solver-elastic (S5 drops it).
- **Work:** `assert-crate-dag.sh` is not edited.
- **Signal:**
  - `cargo test -p reify-build-utils --test crate_dag_assertion` is green on main.
  - Synthetic metadata fixtures turn it red in four cases, each naming the edge or crate and the rule: a new `kernel-x → eval` edge, a `solver → kernel` edge, an unclassified crate, and a stale exception.
  - `bash scripts/verify-pipeline-guard.sh requires-full-gate scripts/assert-crate-strata.py` exits 1 (re-checked before landing).

### δ₁ (#8959) — Split the contract suite: core, taxonomy modes, B-rep extension
- **Keys:** none (fk-e6a266a72f41 closes in δ₂).
- **Files:** `crates/reify-test-support/src/kernel_assertions.rs`, `crates/reify-test-support/src/kernel_assertions/{core,brep,modes,self_tests}.rs` (new).
- **Deps:** β₁ (it deletes `assert_stub_kernel_errors!` from the same file, so the split edits the final content).
- **Work:**
  - New macro arms: `core; $factory, fixture = …, probe = …` and `strict_taxonomy; $factory`.
  - `real;` is re-expressed from the same helpers, with generated test names unchanged.
  - `assert_dangling_reference_taxonomy` is extended to export and tessellate; OCCT's handle tests already prove those variants, so its "unverified variants" caveat goes.
  - Negative self-tests for each new check.
  - Run the h14 protocol and record the verdict in the commit message.
- **Signal:** `cargo test -p reify-test-support kernel_assertions` is green, including `core_catches_*` and `strict_taxonomy_catches_*`. On a real surface: OCCT's generated `kernel_contract_real_*` tests keep their names and stay green (`cargo test -p reify-kernel-occt kernel_contract_real`, tallied once).

### δ₂ (#8960) — Instantiate the core on every real kernel
- **Keys:** fk-e6a266a72f41. **INV:** INV-GEO-4 (pointers appended inline).
- **Files:** `crates/reify-kernel-{fidget,manifold,openvdb,gmsh}/tests/kernel_contract.rs` (new); `crates/reify-kernel-fidget/src/kernel.rs` and `crates/reify-kernel-manifold/src/kernel.rs` (delete the bespoke Send+Sync tests and stale macro comments).
- **Deps:** δ₁, β₁, #8529.
- **Work:**
  - Fidget: `core;` with a Box fixture.
  - Manifold: `core;` with an `ingest_mesh(unit_cube)` fixture and a `SurfaceArea` probe.
  - OpenVDB: `core;` with an ingest fixture.
  - gmsh: `core;` plus `unsupported("Gmsh")`. This file is born-then-deleted: meshing-service ε #8288 deletes it along with gmsh's impl (L17).
  - None of these is heavy or in the `occt` nextest group, so no nextest partition or `run-all-classification` row is needed; say so in the commit message.
- **Signal:** `git grep -nE 'assert_kernel_contract!\((core|real)' crates/reify-kernel-*` has one hit per kernel crate. The four binaries run with a non-zero test count under the gate.

### δ₃ (#8961) — Strict dangling-handle taxonomy (L6)
- **Keys:** none.
- **Files:** `crates/reify-ir/src/geometry/query.rs` (`GeometryQuery::primary_handle()`, exhaustive), `crates/reify-test-support/src/mocks.rs` (its private `primary_handle` deleted), `crates/reify-kernel-fidget/src/kernel.rs`, `crates/reify-kernel-manifold/src/kernel.rs`, `crates/reify-kernel-openvdb/src/kernel_real.rs`, the three `kernel_contract.rs` files.
- **Deps:** δ₂, α₃.
- **Work:**
  - Fidget checks the primary handle before answering and returns `ExportError::InvalidHandle`; rewrite its 4 bespoke pins.
  - Manifold's Distance/Contains/GeoEquiv and extract paths return `InvalidHandle`; its export returns `ExportError::InvalidHandle`.
  - OpenVDB returns `InvalidReference` for a dangling execute operand and `TessError::InvalidHandle` for tessellate.
  - Add `strict_taxonomy;` to the three instantiations.
- **Signal:** the `strict_taxonomy` tests are green for Fidget, Manifold and OpenVDB. `git grep -n 'fn primary_handle' crates/reify-test-support` = 0.

### δ₄ (#8962) — MockGeometryKernel under the contract; INV-GEO-4 flips
- **Keys:** none. **INV:** INV-GEO-4 → `enforced(test)` (this leaf owns the status cell; its id is recorded on #8821).
- **Files:** `crates/reify-test-support/src/mocks.rs` (or a sibling `mock_strict.rs` if it grows), `crates/reify-test-support/tests/mock_kernel_contract.rs` (new), `docs/invariants.md`.
- **Deps:** δ₃.
- **Work:**
  - `MockGeometryKernel::strict_handles()` (opt-in; the default is unchanged) rejects ids the mock did not mint or seed, using the invalid-handle variants.
  - Instantiate `core;` and `strict_taxonomy;` on it.
  - Rewrite INV-GEO-4 per §11's row text.
- **Signal:** `mock_kernel_contract` is green. The diff touches none of the 135 files that reference MockGeometryKernel outside test-support. The INV-GEO-4 row's pointers resolve.

### δ₆ (#8963) — Shared test-double building blocks
- **Keys:** none.
- **Files:** `crates/reify-test-support/src/kernel_doubles.rs` (new: `AllErrorKernel`, and `ObservingKernel<K>`, which forwards every trait method (28 once #8284 lands), with up to four hook closures; ε₂ later converts its capability half to `forward_kernel_capabilities!`), `crates/reify-test-support/src/kernel_assertions.rs` (the private `impl_all_error_kernel!` delegates to the public one), `tests/infra/adoption-ratchet.manifest`.
- **Deps:** δ₁, #8533.
- **Signal:** `ObservingKernel`'s parity test enumerates the trait the same way #8533's guard does and asserts every method is forwarded. `python3 tests/infra/adoption_ratchet.py` prints the new rows at their baselines.
- **Ratchet:** `bespoke-kernel-double-eval | ^ *impl(<[^>]*>)? +([A-Za-z_:]+::)?GeometryKernel for  | crates/reify-eval/**/*.rs | <baseline 47> | δ₇`; `bespoke-kernel-double-geometry` over `crates/reify-geometry/**/*.rs` (3) and `bespoke-kernel-double-mesh-morph` over `crates/reify-mesh-morph/**/*.rs` (3).

### δ₇ (#8964) — Migrate the bespoke test doubles
- **Keys:** fk-7bfef234d79e.
- **Files:** `[]` (the 52 doubles the census lists; the ledger is the first deliverable), `tests/infra/adoption-ratchet.manifest`.
- **Deps:** δ₆, δ₄.
- **Work:**
  - A double is **replaceable** iff each method it overrides is (a) all-error, (b) a Mock configuration (canned query/extract/volume results, recorded ops, canned tessellation mesh or error, canned export result) or (c) `ObservingKernel` with at most four hooks over an existing kernel.
  - Not replaceable: reify-ir's trait-default fixtures (dependency direction), the contract self-test fixtures, and doubles whose hook is the subject under test (`HookAdvertisingKernel`, `HookKernel`, `OrderingKernel`, `HistoryMockKernel` ×2, `DropInjectingKernel` ×2, `SplitMockKernel`).
  - The three 16-method "forward everything" recorders become `ObservingKernel`s.
- **Signal:** the ratchet ceilings are lowered in the same diff to the ledger's measured residual (estimated reify-eval about 15, reify-geometry 1 (the holder), reify-mesh-morph 1 or fewer). The ledger is committed in the commit message.

### ε₁ (#8965) — Capability sub-traits and accessors beside the flat methods
- **Keys:** none (fk-047cb467bd07 closes in ε₂).
- **Files:** `crates/reify-ir/src/geometry/kernel.rs`, `crates/reify-ir/src/lib.rs`, `crates/reify-kernel-occt/src/handle.rs` (trait and sub-trait impl blocks only, R1), `crates/reify-kernel-manifold/src/kernel.rs`, `crates/reify-kernel-gmsh/src/kernel_real.rs`, `crates/reify-kernel-openvdb/src/kernel_real.rs`, `crates/reify-geometry/src/lib.rs`, `crates/reify-geometry/tests/forwards_every_kernel_method.rs`.
- **Deps:** α₃, #8533, #8672, #8284, β₁.
- **Work:**
  - Define the eleven new sub-traits (§4.1, including `TopologyTessellation` for #8284's `tessellate_with_topology`), `KernelCapability` (strum `EnumIter`) and `has_capability`.
  - Add the eleven new accessors with transitional `None` defaults (`attribute_hook` already exists).
  - Real kernels implement the sub-traits, and their flat overrides become one-line forwards, so each body has one home. The holder forwards each accessor.
- **Signal:** `forwards_every_kernel_method` is green with the accessors enumerated by its syn oracle. Each real kernel crate gains a unit test asserting that its `Some` accessor set equals its override set; Manifold, for example, returns Some for topology_extraction, mesh_ingest, optioned_export and attribute_hook.

### ε₂ (#8966) — Required core and accessors, forwarding macros, delegation lint (INV-GEO-5, partial)
- **Keys:** none (fk-047cb467bd07 closes in ε₅). **INV:** INV-GEO-5, the new row for program invariant candidate 6. It flips to `partial: enforced(type+lint) for the capability accessors and core methods, proposed for the flat capability methods ε₃–ε₅ remove`, and ε₅ completes it. Enforcement: the compiler (required accessors and core), the `forward_kernel_capabilities!` unit test, and the workspace lint (BT-ε2). This is a four-site lockstep: `docs/invariants.md`, `docs/legibility/design-invariants.md` (one entry), the program doc pointer (the lead) and one CLAUDE.md Pointers line. The leaf id is recorded on #8821.
- **Files:** `crates/reify-ir/src/geometry/kernel.rs`, `crates/reify-ir/src/lib.rs`, `crates/reify-geometry/src/lib.rs`, `crates/reify-geometry/tests/kernel_wrappers_forward_every_capability.rs` (new), every file holding an `impl GeometryKernel for` (enumerated at start; about 41–46 blocks remain after δ₇), the kernel crates (an explicit `reset` each), `docs/invariants.md`, `docs/legibility/design-invariants.md`, `CLAUDE.md`, `crates/reify-eval/tests/holder_forwards_real_occt.rs` (new; BT-ε3; reify-eval is in the nextest `occt` group by package, so no registration is needed, and the commit message says so).
- **Deps:** ε₁, δ₇, β₁.
- **Work:**
  - Remove the default bodies of the accessors and of `reset`, so the five core methods and every accessor are required.
  - The flat capability methods that ε₃–ε₅ still have to remove keep their defaults for now. They sit on a named shrinking list in the lint: 21 at ε₂, plus #8284's `tessellate_with_topology`.
  - Add the two macros and apply them across all impls.
  - Write the delegation lint with L1's rule. Its seeded violations are inline source strings.
  - Manifold, OpenVDB, Fidget and Gmsh each state their own `reset`: either clear the table, or a no-op with its reason.
- **Signal:**
  - BT-ε1, BT-ε2 and BT-ε3 are green. BT-ε2 includes the seeded opt-out and hand-written-`None` wrappers.
  - The lint asserts that no core method or accessor has a default body, and that the flat list holds exactly its named members.
  - `git grep -c 'fn reset' -- crates/reify-kernel-{manifold,fidget,openvdb}/src crates/reify-kernel-gmsh/src/kernel_real.rs` is at least 1 in each.

### ε₃ (#8967) — Topology extraction and multi-body leave the flat trait; the `kernel_calls` facade
- **Keys:** none.
- **Files:** `crates/reify-ir/src/geometry/kernel.rs`, `crates/reify-ir/src/kernel_calls.rs` (new), `crates/reify-ir/src/lib.rs`, `crates/reify-geometry/src/lib.rs`; eval callers `crates/reify-eval/src/{engine_build,feature_datum,geometry_ops,primitive_attribute_seed,selector_vocabulary_v2,topology_selectors}.rs` and `crates/reify-eval/src/compute_targets/bc_resolve.rs` (paths as they stand after S13); `crates/reify-test-support/src/{kernel_assertions,mocks}.rs`; `crates/reify-kernel-occt/src/handle.rs`, `crates/reify-kernel-manifold/src/kernel.rs`; test impls overriding these methods; `tests/infra/adoption-ratchet.manifest`.
- **Deps:** ε₂.
- **Work:**
  - Switch 63 eval and 3 test-support call sites to the facade.
  - Delete the five flat methods.
  - The "topology extraction not supported" text gets one home.
- **Signal:** `git grep -c '"topology extraction not supported' -- crates` = 1. The flat `fn extract_(edges|faces|vertices)(` exists only inside `trait TopologyExtraction`. `reify eval examples/topology_selectors/fillet_top_edges.ri` is byte-identical before and after.
- **Ratchet:** `flat-extract-call | ^[^/]*[.]extract_(edges|faces|vertices)[(] | crates/reify-eval/src/**/*.rs | 0 | ε₃` (the facade spells `kernel_calls::extract_*(k, h)` without a leading `.`; the leaf confirms the pattern misses it and catches `compute_targets/bc_resolve.rs`).

### ε₄ (#8968) — Mesh, voxel and volume-mesh capabilities leave the flat trait
- **Keys:** none.
- **Files:** `crates/reify-ir/src/geometry/kernel.rs`, `crates/reify-ir/src/kernel_calls.rs`, `crates/reify-geometry/src/lib.rs`, `crates/reify-eval/src/{engine_build,realize_solid_sdf,realization_content}.rs`, `crates/reify-kernel-manifold/src/kernel.rs`, `crates/reify-kernel-gmsh/src/kernel_real.rs`, `crates/reify-kernel-openvdb/src/kernel_real.rs`, `crates/reify-test-support/src/mocks.rs`, and the test impls overriding these methods.
- **Deps:** ε₃ (the `kernel_calls` facade).
- **Work:**
  - Switch the 14 eval sites.
  - `surface_options_content_hash` callers use `voxel_surfacing().map_or(NO_OPTIONS, …)`.
  - Delete the ten flat methods and remove them from the lint's shrinking list.
  - If meshing-service ε #8288 has already deleted gmsh's volume-meshing methods, `VolumeMeshing` moves only what remains, or is not created (L17).
- **Signal:** the flat names exist only inside their sub-traits. `reify build examples/multi_kernel/voxel_to_mesh.ri -o x.stl` reports the same `Triangles:` as before the leaf (≥ 10000 once #6308 has landed). `reify eval` on `examples/multi_kernel/manifold_boolean.ri` is unchanged.

### ε₅ (#8969) — Probe, history, batch query, optioned export and topology tessellation leave the flat trait (INV-GEO-5 complete)
- **Keys:** fk-047cb467bd07. **Priority: high** (a dependency of #8612). **INV:** INV-GEO-5 → `enforced(type+lint)` (the leaf id is recorded on #8821 as the completing owner).
- **Files:** `crates/reify-ir/src/geometry/kernel.rs`, `crates/reify-ir/src/kernel_calls.rs`, `crates/reify-geometry/src/lib.rs`, `crates/reify-geometry/tests/kernel_wrappers_forward_every_capability.rs`, `crates/reify-eval/src/{engine_build,topology_selectors,geometry_ops,kernel_attribute_hook}.rs` (post-S13 paths), `crates/reify-mesh-morph/src/boundary.rs`, `crates/reify-test-support/src/kernel_assertions.rs`, `crates/reify-kernel-occt/src/handle.rs`, `crates/reify-kernel-manifold/src/kernel.rs`, `docs/invariants.md`, the callers of `tessellate_with_topology` (enumerated at start), and test impls.
- **Deps:** ε₄, η₂ (the typed Point reply that `closest_point_on_shape`'s callers move to).
- **Work:**
  - Switch the remaining call sites to `kernel_calls`.
  - Delete the flat methods:
    - probe, history, batch and export;
    - #8284's flat `tessellate_with_topology`, with its callers moved to `kernel_calls::tessellate_with_topology`;
    - `closest_point_on_shape`, whose KernelProjector caller moves to `query(ClosestPointOnShape)` with the typed Point (L3).
  - The lint's shrinking list becomes empty and is deleted.
  - The export fallback returns an `ExportWarning` naming the ignored options (L2).
- **Signal:**
  - The ε₂ lint asserts that `trait GeometryKernel` has zero items with a default body and exactly five non-accessor functions.
  - `reify build` of a STEP-export example with options produces identical bytes.
  - A 3MF export through a kernel without `OptionedExport` reports the ignored-options warning. This is pinned by a unit test over a mock kernel, plus the CLI line if a front end prints export warnings.
  - INV-GEO-5 reads `enforced(type+lint)`.

### ζ₁ (#8970) — The mesh witness: tol, weld, waiver; two entry points; one function per obligation
- **Keys:** none (fk-a01d863c73ba closes in ζ₂).
- **Files:** `crates/reify-ir/src/geometry/mesh.rs`, `crates/reify-eval/src/engine_build.rs` (the `validate` call → `check_mesh_contract`), `crates/reify-kernel-gmsh/src/mesh_boundary.rs` (`validate` → `check_mesh_contract`), and the external test `.validate(` sites (14, by rule `git grep -E '\.validate\('` outside reify-ir excluding `solvespace`).
- **Deps:** α₂.
- **Work:** as in §4.3. `check_mesh_contract_welded` stays until ζ₂.
- **Signal:** `git grep -c -E 'pub fn validate\b' crates/reify-ir/src/geometry/mesh.rs` = 0. The `check_contract` body is at most 15 lines. BT-ζ1 is green.

### ζ₂ (#8971) — Mesh-consuming methods take `&ValidatedMesh`; one front door; posture in the witness (R3)
- **Keys:** fk-a01d863c73ba, fk-a60cf0fbe772, fk-a0ce1853f2da. **INV:** INV-GEO-1 (mechanism cell rewritten per §11; its id is recorded on #8821).
- **Files:**
  - reify-ir: `crates/reify-ir/src/geometry/kernel.rs` (signatures of `ingest_mesh`, `ingest_mesh_at_resolution`, `register_mesh_handle`, `mesh_surface_to_volume`, `mesh_surface_to_volume_attributed` in their sub-traits), `crates/reify-ir/src/geometry/mesh.rs` (`check_mesh_contract_welded` deleted).
  - Wrapper and kernels: `crates/reify-geometry/src/lib.rs`, `crates/reify-kernel-manifold/src/kernel.rs`, `crates/reify-kernel-openvdb/src/kernel_real.rs`, `crates/reify-kernel-gmsh/src/kernel_real.rs`, `crates/reify-kernel-gmsh/src/mesh_boundary.rs`.
  - reify-eval: `crates/reify-eval/src/engine_build.rs`, `crates/reify-eval/src/engine_build/mesh_handoff.rs` (new), `crates/reify-eval/src/realize_solid_sdf.rs`.
  - `crates/reify-test-support/src/lib.rs`: a `validated(mesh)` helper that calls `into_validated(0.0, Enforce)`; no second public mint.
  - The test impls and calls, `docs/invariants.md`, `tests/infra/adoption-ratchet.manifest`.
- **Deps:** ζ₁, ε₄, #6308.
- **Work:**
  - §4.3. MarchingCubes output keeps today's `!from_marching_cubes` exemption at the conversion handoff; ζ₄ owns removing it.
  - INV-GEO-1's coverage clause names the exemption as open, with ζ₄ as the owner.
  - gmsh's inherent `mesh_to_volume` / `refine_volume_with_size_field`, which reify-solver-elastic calls with `&Mesh` outside the trait, are recorded in the cell as the remaining gap. That gap closes with meshing-service β/ζ.
- **Signal:**
  - No `&Mesh` remains in the five signatures, and `ValidatedMesh` appears in all five.
  - `git grep -c 'static MODE' crates/reify-kernel-manifold/src/kernel.rs` = 0.
  - `MeshContractMode::from_env` has one production caller.
  - BT-ζ2 and BT-ζ3 are green.
- **Ratchet:** `mesh-mint-eval | ^[^/]*[.]into_validated[(] | crates/reify-eval/src/**/*.rs | 1 | ζ₂`, and `mesh-mint-kernels | ^[^/]*[.]into_validated[(] | crates/reify-kernel-*/src/**/*.rs | 0 | ζ₂` (the kernels' inline test modules mint through `reify_test_support::validated`). `hand-rolled-mesh-length-check | ^[^/]*is not a multiple of 3 | crates/reify-kernel-openvdb/src/**/*.rs | <post-leaf, measured> | ζ₂`.

### ζ₃ (#8972) — ElementOrderTag owns the degree and tet-stride tables
- **Keys:** none (fk-92d313d2986f is stamped onto #8285, R4).
- **Files:** `crates/reify-ir/src/geometry/volume_mesh.rs`, `crates/reify-kernel-gmsh/src/fill_metrics.rs`, `crates/reify-kernel-gmsh/src/through_thickness.rs`, `crates/reify-kernel-gmsh/src/tet_readback.rs` (after #7636; else `init.rs`), `crates/reify-kernel-gmsh/src/kernel_real.rs`, `crates/reify-kernel-gmsh/src/refine_volume.rs`, `crates/reify-kernel-gmsh/src/mesh_boundary.rs` (the three `Mesh.ElementOrder` option values), `tests/infra/adoption-ratchet.manifest`.
- **Deps:** #7636, α₂.
- **Work:**
  - Add `ElementOrderTag::polynomial_degree()` and `nodes_per_tet()`; `VolumeMesh::nodes_per_element` delegates to them.
  - The gmsh element-type codes (4/11) stay one local table.
  - `cache_key.rs`'s 0/1 discriminant stays: it is a frozen serialization value.
  - The gmsh call sites are partly born-then-deleted under meshing-service β/ε (L17). The reify-ir methods survive, and β's `run_mesh` uses them.
- **Signal:** the real-gmsh P1 and P2 cases in `crates/reify-kernel-gmsh/tests/mesh_to_volume_tests.rs` produce node and element counts that are captured at the base commit and unchanged after the leaf (a real gmsh mesh, not synthetic input). `nodes_per_tet_matches_volume_mesh_nodes_per_element` is green.
- **Ratchet:** `gmsh-element-order-match | ^[^/]*ElementOrderTag::P1 => | crates/reify-kernel-gmsh/src/**/*.rs | 1 | ζ₃`. It matches 7 lines today; the 1 left is `cache_key.rs`.

### ζ₄ (#8973) — MarchingCubes output passes the front door
- **Keys:** none. **INV:** INV-GEO-1 (coverage clause: the MarchingCubes exemption is closed).
- **Files:** `crates/reify-eval/src/engine_build.rs`, `crates/reify-eval/src/engine_build/mesh_handoff.rs`, `crates/reify-eval/tests/voxel_to_mesh_e2e.rs`, `docs/invariants.md`.
- **Deps:** ζ₂, #6308.
- **Work:**
  - **Measure first (G6).** Run `examples/multi_kernel/voxel_to_mesh.ri`'s MarchingCubes output through `check_mesh_contract` at its producer tolerance (0.0), and do the same for `isosurface(cylinder(8mm, 20mm))`. Record the verdicts in the commit message.
  - If both pass, delete the `!from_marching_cubes` exemption: MarchingCubes output passes through `admit_mesh` under the engine posture, and `register_mesh_handle` receives a minted witness.
  - If either fails, **stop and escalate** the measured witness (invariant, counts) as a premise blocker. A producer fix is a human-filed task, not this leaf's scope, and nothing here widens.
- **Signal:**
  - `git grep -n '!from_marching_cubes' crates/reify-eval/src` = 0. The `from_marching_cubes` binding itself stays, because it still picks `register_mesh_handle` over `ingest_mesh`.
  - `voxel_to_mesh_e2e.rs` asserts that the MarchingCubes mesh passes `check_mesh_contract(0.0)`.
  - `reify build examples/multi_kernel/voxel_to_mesh.ri -o x.stl` exits 0 under the default Enforce posture with `Triangles:` ≥ 10000.

### η₁ (#8974) — `GeometryKernel::query` returns `QueryReply` (transitional `Untyped`)
- **Keys:** none.
- **Files:** `crates/reify-ir/src/geometry/query.rs`, `crates/reify-ir/src/geometry/kernel.rs`, `crates/reify-ir/src/lib.rs`, every file with a `fn query(_many)?(` definition (regenerated at start by `git grep -lE "fn query(_many)?\(" -- crates`), the reify-eval production query callers (51 sites in 10 files), `tests/infra/adoption-ratchet.manifest`.
- **Deps:** ε₂, δ₃, δ₇, α₃, #8684, #8751, #8672, #8533.
- **Work:**
  - Add `QueryReply` (§4.4) with `Untyped(Value)`, `QueryReplyKind`, and `GeometryQuery::reply_kind()`/`reply_dimension()` (both exhaustive), plus the accessors.
  - Producers wrap `Untyped`; consumers unwrap through one accessor; mock builders store `Untyped`.
  - No behaviour change.
- **Signal:** `git grep -nE 'fn query(_many)?\(.*Result<(Vec<)?(reify_ir::)?Value' -- crates` = 0. This is a one-way signal: about 21 of the 99 definitions have multi-line signatures and need a type check to see them, which the compiler provides. BT-η2 is byte-identical.
- **Ratchet** (L12 rules; baselines measured at η₁'s base commit):
  - `query-reply-untyped | ^[^/]*QueryReply::Untyped[(] | crates/**/*.rs | <count> | η₆`;
  - `query-reply-json-literal-eval | ^[^/]*[{]\\?["](x|xmin)\\?["]: | crates/reify-eval/**/*.rs | <count> | η₃`, with `-occt`, `-manifold` and `-test-support` rows over `crates/reify-kernel-occt/**/*.rs`, `crates/reify-kernel-manifold/**/*.rs` and `crates/reify-test-support/**/*.rs` (the JSON shape: `{` then an optionally escaped quote, so the reify-compiler `"x":Length` docs do not count, and no backslash sits inside a bracket, which the engine rejects);
  - `query-reply-parser | ^[^/]*fn (parse_flat_number_object|parse_xyz_value|parse_xyz_json|parse_bbox_[a-z_]+|parse_normal_z|parse_kind_string|from_kernel_json|centroid_json|json_xyz|json_bbox)[(] | crates/**/*.rs | <count> | η₃`.

### η₂ (#8975) — Point and direction replies typed
- **Keys:** none (fk-030602c2fd62 and fk-59c46755d24c close in η₃).
- **Files:**
  - producers: `crates/reify-kernel-occt/src/query.rs` (created by #8684), `crates/reify-kernel-manifold/src/{kernel,queries}.rs`;
  - mock: `crates/reify-test-support/src/mocks.rs` (the centroid/CoM/closest-point/edge-tangent/face-normal(-at) builders take `[f64;3]`);
  - consumers: `crates/reify-eval/src/{topology_selectors,selector_vocabulary_v2,dynamics_ops,engine_build,primitive_attribute_seed}.rs`, `crates/reify-eval/src/geometry_ops/{query,pose}.rs`, `crates/reify-eval/src/compute_targets/bc_resolve.rs`;
  - test parsers: `crates/reify-kernel-occt/tests/common/mod.rs`, `crates/reify-kernel-manifold/tests/queries.rs`, `crates/reify-kernel-conformance/tests/common/mod.rs`, `crates/reify-eval/tests/{apply_transform_e2e,centered_primitives_e2e}.rs`, `crates/reify-eval/tests/harness_stress_scenarios/stress_query_consistency.rs`;
  - plus the builder-call files, regenerated by rule.
- **Deps:** η₁.
- **Work:**
  - Centroid, CenterOfMass and ClosestPointOnShape return `Point`; FaceNormal, EdgeTangent and FaceNormalAt return `Direction`, built straight from FFI `Point3` / `[f64;3]`.
  - `reply_to_value` keeps today's surface.
  - Delete `parse_xyz_value`, `parse_xyz_json`, `parse_normal_z`, `centroid_json`, `json_xyz` and the malformed-JSON tests, which cannot be expressed any more.
- **Signal:** those function names have 0 definitions, and the `"x":` JSON-literal count drops to the bbox-only residue. The oracle's `Probe.c`/`Probe.cp` lines are byte-identical.

### η₃ (#8976) — Bounding-box reply typed; the tokenizers and the byte tag go
- **Keys:** fk-59c46755d24c, fk-0375628ba585, fk-030602c2fd62. **Priority: high** (a dependency of #8612).
- **Files:** `crates/reify-kernel-occt/src/query.rs`, `crates/reify-kernel-manifold/src/{kernel,queries}.rs`, `crates/reify-test-support/src/mocks.rs` (`with_bbox_result` takes min/max; `Aabb::from_kernel_json` deleted), `crates/reify-eval/src/{topology_selectors,selector_vocabulary_v2,engine_build,primitive_attribute_seed}.rs`, `crates/reify-eval/src/geometry_ops/query.rs`, `crates/reify-eval/tests/harness_topology_selector/topology_attribute_primitives_direct.rs`, `crates/reify-eval/tests/rounded_primitives_e2e.rs`, `crates/reify-kernel-occt/tests/harness_occt/offset_surface_integration.rs`, plus builder-call files.
- **Deps:** η₂.
- **Work:**
  - BoundingBox returns `QueryReply::BoundingBox`, from `ffi::BBox` and Manifold's min/max.
  - Delete `parse_flat_number_object`, the four `parse_bbox_z/axis` functions, `parse_bbox_all_extents`, `parse_bbox_midpoint`, `parse_bbox_xyz_min`, `parse_bbox_radial_extent`, `json_bbox`, `from_kernel_json` and `Axis::as_byte` (`extremal_by_bbox` reads `bbox.min[axis.index()]`).
  - `dispatch_bounding_box` becomes a `reply_to_value` arm.
- **Signal:** the JSON-literal and parser ratchet rows are at 0. `git grep -n "b'x'" crates/reify-eval/src` = 0. The oracle's `Probe.bb` and `Probe.topb` lines are byte-identical.

### η₄ (#8977) — Kind replies typed; the kind enums name themselves
- **Keys:** none (fk-fdb4e221350a closes in η₆).
- **Files:** `crates/reify-ir/src/geometry_id.rs` (strum derives on FaceSurfaceKind/EdgeCurveKind; `try_from_str` deleted), `crates/reify-kernel-occt/src/query.rs`, `crates/reify-eval/src/selector_vocabulary_v2.rs`, `crates/reify-eval/src/feature_datum.rs`, `crates/reify-eval/src/geometry_ops/args.rs`, `crates/reify-test-support/src/mocks.rs`, `crates/reify-eval/tests/harness_topology_selector/selector_vocabulary_v2_mock.rs`, `crates/reify-kernel-occt/tests/harness_occt/offset_solid_integration.rs`, builder-call files.
- **Deps:** η₁, #8748.
- **Work:**
  - OCCT decodes the FFI name once with `parse::<FaceSurfaceKind>()`; an unknown name is a `QueryFailed` naming it, so drift fails loudly.
  - `parse_kind_string` goes, and feature_datum matches `CurveKind(Line)`; its silent parse-failure path is gone.
  - The `expected one of …` literals come from `VARIANTS`.
  - A unit test asserts that `VARIANTS` lists every `EnumIter` name.
- **Signal:** `git grep -n 'fn parse_kind_string\|fn try_from_str' -- crates` = 0, and `git grep -n "BSplineSurface/\|BSplineCurve/" crates/reify-eval/src` = 0. The oracle's `faces_by_surface_kind` "Plain" warning line is byte-identical. `examples/selectors/selector_vocabulary_v2_leaves.ri` output is unchanged.

### η₅ (#8978) — The remaining replies typed
- **Keys:** none.
- **Files:** `crates/reify-kernel-occt/src/query.rs`, `crates/reify-kernel-manifold/src/kernel.rs`, `crates/reify-test-support/src/mocks.rs`, `crates/reify-eval/src/geometry_ops/` children (post-S13 names), `crates/reify-eval/src/{feature_datum,dynamics_ops,engine_eval,engine_constraints,topology_selectors,selector_vocabulary_v2}.rs`, builder-call files.
- **Deps:** η₁.
- **Work:**
  - Real, Bool, Indices, Handle, Matrix3, PrincipalCurvatures and Datum replies are typed.
  - The dual-shape arms (`Real | Scalar`) and the non-Bool/non-numeric fallbacks collapse into the accessors.
  - `ShapeLocalTolerance` returns `Real`.
  - `datum_from_value`, `parse_curvature_matrix_reply` and the inertia List walk become typed reads.
- **Signal:** `git grep -n 'JSON encoding\|Returns `Value::' crates/reify-ir/src/geometry/query.rs` = 0. The oracle and S13's O1–O3 are unchanged.

### η₆ (#8979) — Delete `Untyped`; the reply-kind contract
- **Keys:** fk-fdb4e221350a.
- **Files:** `crates/reify-ir/src/geometry/query.rs`, `crates/reify-test-support/src/kernel_assertions/core.rs`, `tests/infra/adoption-ratchet.manifest`.
- **Deps:** η₂, η₃, η₄, η₅.
- **Work:** delete `QueryReply::Untyped` and the transitional accessor. The contract core asserts `reply.kind() == query.reply_kind()` for every answered query (BT-η1).
- **Signal:** `git grep -n 'Untyped' -- crates/reify-ir/src` = 0. Every reply ratchet row is at 0. BT-η1 is green on OCCT and Manifold.

### θ₁ (#8980) — Length and Angle; the primitives family
- **Keys:** none (fk-1d857a1e9750 closes in θ₄).
- **Files:** `crates/reify-ir/src/quantity.rs` (new), `crates/reify-ir/src/lib.rs`, `crates/reify-ir/src/geometry/op.rs`, `crates/reify-ir/Cargo.toml` (`syn` dev-dep), `crates/reify-ir/tests/geometry_op_quantity_fields.rs` (new), `crates/reify-eval/src/geometry_ops.rs` (or its post-S13 children: `accept_length_value`, `accept_angle_value`, `required_length_values`, the `prim_*` builders, `prim_half_space`), `crates/reify-kernel-occt/src/{op_args,lib}.rs` (the primitive arms; `op_args.rs` is created by #8652), `crates/reify-kernel-fidget/src/kernel.rs`, `tests/infra/adoption-ratchet.manifest`, plus primitive test sites by rule (`git grep -l -E 'GeometryOp::(Box|Cylinder|Sphere|Tube|Cone|Wedge|Torus|HalfSpace) \{'`).
- **Deps:** α₃, #8652, #8669, θ₅ (θ₁ reads HalfSpace's normal through θ₅'s `required_dimensionless_arg`; one direction reader).
- **Work:**
  - Add the types (§4.5), rebase the eval gate onto `accept`, and type Box..Torus and HalfSpace px/py/pz as `Length`.
  - HalfSpace nx/ny/nz become `f64` read through θ₅'s `required_dimensionless_arg`.
  - Kernels read `.metres()`.
  - Fidget's `extract_length_f64` loses all its callers and goes.
- **Signal:** `reify eval tests/prd-gate/fixtures/kernel_interface_halfspace_nonnumeric_normal.ri` names `half_space` and `nz` instead of `expected numeric value`. `value_fields_in_geometry_op_within_baseline` moves from 47 to 26. `git grep -c 'extract_length_f64(' crates/reify-kernel-fidget/src` = 0.
- **Ratchet:** the syn baseline in `geometry_op_quantity_fields.rs`, plus `raw-quantity-ctor | ^[^/]*(Length::from_metres|Angle::from_radians)[(] | crates/reify-eval/src/**/*.rs | <allowlist count> | θ₄` (production SI sources only: engine posing, `decode_plane`, AffineMap decode).

### θ₂ (#8981) — Modify, sweep and profile families; SweptKind
- **Keys:** none.
- **Files:** `crates/reify-ir/src/geometry/op.rs`, `crates/reify-eval/src/geometry_ops.rs` (modify, sweep and profile builders, or their post-S13 children), `crates/reify-eval/src/sweep_classifier.rs`, `crates/reify-kernel-occt/src/lib.rs` (and `execute.rs` after S11 ι), plus tests by rule.
- **Deps:** θ₁.
- **Work:**
  - Fillet, Chamfer, ChamferAsymmetric, Shell, Thicken, ZoneSlab, OffsetSolid, OffsetSurface and OffsetCurve become `Length`; Draft.angle becomes `Angle`. The history path's reads move with them.
  - Extrude, ExtrudeSymmetric and Pipe become `Length`, and so do the Rectangle, Circle and Ellipse profiles.
  - `SweptKind::Extrude.length: Length`, so the sweep_classifier `as_f64` decodes go.
- **Signal:** the syn baseline moves from 26 to 7. `git grep -n 'as_f64()' crates/reify-eval/src/sweep_classifier.rs` = 0.

### θ₃ (#8982) — Pattern family; the tripwire is retired
- **Keys:** fk-08aa3fff3378. **Subsumes #8795.**
- **Files:** `crates/reify-ir/src/geometry/op.rs`, `crates/reify-ir/src/kernel_validation.rs`, `crates/reify-ir/src/lib.rs`, `crates/reify-eval/src/geometry_ops.rs` (pattern builders, and the D3 "rejected alternative" doc rewritten), `crates/reify-kernel-occt/src/{op_args,lib}.rs`, `crates/reify-kernel-occt/tests/harness_occt/{execute_with_history_lowering,inline_pattern_ratchet}.rs` (created by #8652; θ₃ is downstream of it through θ₁), plus tests by rule.
- **Deps:** θ₂.
- **Work:**
  - LinearPattern, LinearPattern2D and CircularPattern: spacing becomes `Length` and angle becomes `Angle`.
  - Delete `extract_length_f64`, `extract_f64`, `check_length_field`, `got_label`, `value_variant_name`, both kernel-field message formatters, the arming triple, `define_kernel_length_extract!` and the re-exports, along with their tests and the false "cannot be shared" rationale.
  - Rewrite #8652's tripwire-pinning harness tests to assert typing, and rewrite its ratchet row (ii) message.
  - Port #8795's content first.
- **Signal:** the syn baseline is at 0. `git grep -nE 'extract_length_f64|check_length_field|define_kernel_length_extract|arm_length_tripwire_assert' -- crates` = 0. `git grep -n '`arg_acceptance` lives in' crates/reify-ir/src/kernel_validation.rs` = 0; that is the false rationale's wording at main.

### θ₄ (#8983) — The bare-f64 length and angle fields; the justification guard
- **Keys:** fk-1d857a1e9750.
- **Files:** `crates/reify-ir/src/geometry/op.rs`, `crates/reify-ir/tests/geometry_op_quantity_fields.rs`, `crates/reify-eval/src/geometry_ops.rs` (transform, revolve, curve, isosurface and polygon builders; `try_eval_topology_selector`, `eval_kinematic_on_snapshot`, `walk_placed_realizations`, `decode_plane`; or their post-S13 children), `crates/reify-eval/src/engine_build.rs` (Surface), `crates/reify-kernel-occt/src/{lib,op_args}.rs`, `crates/reify-kernel-openvdb/src/kernel_real.rs` (Surface, ApplyTransform), plus tests by rule (`git grep -l -E 'GeometryOp::(Translate|Rotate|RotateAround|Revolve|LineSegment|Arc|Helix|Surface|Mirror|Split|ApplyTransform|AffineApply|ArbitraryPattern|InterpCurve|BezierCurve|NurbsCurve|PolygonProfile|NurbsSurface) \{'`). This may split into transforms and patterns versus curves and surfaces if the leaf is too large.
- **Deps:** θ₃.
- **Work:**
  - Scalar lengths become `Length`, points become `[Length;3]`/`Vec<[Length;3]>`, and angles become `Angle`.
  - The syn guard requires every remaining `f64`/`[f64;N]`/`usize` field to carry a row in a closed justification taxonomy (Direction, Quaternion, LinearMap, Factor, Weight, Knot, Count, Degree, Index, Flag), in the style of `units_length_closure_guard.rs`'s `Justification`.
- **Signal:** the guard is green; dropping a row or adding an unjustified `f64` length field turns it red. `examples_smoke` is green.

### θ₅ (#8985) — Dimensionless positions are dimension-checked (user-visible)
- **Keys:** none (a new defect, found and probed this session).
- **Files:** `crates/reify-eval/src/geometry_ops.rs` (or its post-S13 `args.rs`: a `required_dimensionless_arg` over `accept_arg(&v, &dimensionless_spec())`, used at factor, count and direction positions); docs-truth (overlay, all four items): `crates/reify-mcp/src/tools/chunks/geometry.md`, `crates/reify-mcp/src/tools/chunks/units.md`, `examples/best_practices/dimensioned_arguments.ri`, `examples/best_practices/INDEX.md`, `.claude/skills/reify-design/SKILL.md`; plus tests.
- **Deps:** none.
- **Work:**
  - Route Scale.factor, ScaleNonUniform, the pattern counts and the direction components through `dimensionless_spec`. HalfSpace's normal is a raw `Value` passthrough today and is θ₁'s, which reuses this reader. A dimensioned value is a `DimensionedArgRejected` Error; Real, Int and Scalar{DIMENSIONLESS} are accepted.
  - This reverses a documented posture, so the docs change in the same diff:
    - the geometry chunk's "The dimensionless column is an authoring rule you uphold … Measured 2026-08-30: `scale(g, 2mm)` is accepted …" paragraph becomes the rejection, and the transform and pattern sections say a scale factor and a count are plain numbers;
    - the units chunk's matching "authoring rule you uphold, not one the compiler enforces" sentence (units.md, the dimensionless-slot paragraph) is rewritten the same way;
    - `dimensioned_arguments.ri`'s header comment ("the gate runs in ONE direction only") becomes "both directions";
    - its INDEX.md row ("an authoring rule you uphold, not one the compiler enforces") becomes "compiler-enforced in both directions";
    - the reify-design SKILL.md index line names the two-way gate.

  No new fence is added.
- **Signal:**
  - `reify eval tests/prd-gate/fixtures/kernel_interface_scale_dimensioned_factor.ri` exits 1 with a `DimensionedArgRejected` error naming `scale`, while `v_good` still prints `0.008… m^3`.
  - `kernel_interface_pattern_dimensioned_count.ri` exits 1 with an error naming `linear_pattern`.
  - `reify eval examples/best_practices/dimensioned_arguments.ri` stays green; its only `scale` call is bare.
  - `git grep -n 'authoring rule you uphold' -- crates/reify-mcp/src/tools/chunks examples/best_practices` = 0.
  - `examples_smoke` is green. Discoverability: an author asking "why is my scale factor rejected" finds the chunk line and the exemplar.

### ι₁ (#8987) — KNOWN_V02_KERNELS derived from KernelId; typed pragma
- **Keys:** fk-57ffd6bd2af2. **Priority: low.**
- **Files:** `crates/reify-core/src/kernel.rs`, `crates/reify-compiler/src/module_pragmas.rs`, `crates/reify-compiler/src/types.rs`, `crates/reify-compiler/src/compile_builder/ctx.rs`, `crates/reify-test-support/src/builders/module.rs`, `crates/reify-eval/src/engine_build.rs` (the three `with_prefer_kernel` sites), `crates/reify-compiler/tests/harness_compilation_surface/pragma_compile_tests.rs`, `crates/reify-eval/tests/pragma_override_routing.rs`, `tests/infra/adoption-ratchet.manifest`.
- **Deps:** none.
- **Work:**
  - `KernelId::is_pragma_target()` is an exhaustive match whose doc states the gmsh exclusion once (a mesher, not a geometry-kernel candidate).
  - The accepted set is `KernelId::ALL` filtered by it.
  - `kernel_pragma: Option<KernelId>` (no verbatim consumer exists).
  - The doc pointer is corrected to `docs/reify-implementation-architecture.md` §10.8.
  - Meshing-service ε #8288 deletes `KernelId::Gmsh`, which removes the predicate's gmsh arm; the predicate stays (L17).
- **Signal:** `#kernel(gmsh)` prints the byte-identical `unknown kernel 'gmsh'; v0.2 supports {fidget, manifold, occt, openvdb}`. `pragma_targets_are_kernel_id_all_minus_meshers` iterates `KernelId::ALL`.
- **Ratchet:** `kernel-name-literal-set | ^[^/]*"(fidget|gmsh|manifold|occt|openvdb)" | crates/reify-compiler/src/module_pragmas.rs | 0 | ι₁` (the two comment mentions do not count).

### ι₂ (#8989) — OCCT declares what it executes; descriptor completeness
- **Keys:** none.
- **Files:** `crates/reify-kernel-occt/src/register.rs`, `crates/reify-kernel-conformance/tests/descriptor_completeness.rs` (new), `crates/reify-kernel-conformance/Cargo.toml` (adds the reify-kernel-fidget and reify-kernel-openvdb dev-deps, so every registration is linked; ι₆ reuses them).
- **Deps:** none.
- **Work:**
  - Add `(TransformAffineApply, BRep)` and `(SurfaceNurbs, BRep)`.
  - A test asserts that every `Operation` emitted by `geometry_op_to_operation` is declared by some registered kernel (`Operation::iter()` minus a commented allow-list).
  - reify-kernel-conformance is in the nextest `occt` test group by crate, so no registration is needed; say so in the commit message.
- **Signal:** `every_emitted_operation_is_declared_by_some_kernel` is green with all five kernels linked. `reify build examples/affine_tapered_spacer.ri -o x.step` gives rc 0.

### ι₃ (#8992) — KernelSet keyed by KernelId; the sentinel goes
- **Keys:** none.
- **Files:** `crates/reify-eval/src/kernel_set.rs` (new), `crates/reify-eval/src/{lib,engine_admin,engine_build,engine_constraints,engine_eval,geometry_ops,realization_content,realize_solid_sdf,kernel_registry,dispatcher,measure_min_feature,measure_min_wall,realization_read_test_support}.rs`, `crates/reify-eval/src/kernel_owner.rs` (from #6308), `crates/reify-eval/src/engine_build/tests.rs`, every file that calls `with_test_kernels_and_registry` (11 by `git grep -l`, including `crates/reify-eval/src/geometry_op_characterization_probe.rs`), `gui/src-tauri/src/tests/openvdb_kernel_tests.rs`, `crates/reify-eval/tests/ensure_openvdb_kernel.rs`, `tests/infra/adoption-ratchet.manifest`.
- **Deps:** ι₂, #6308.
- **Work:**
  - As in §4.6. `DispatchPlan.kernel: KernelId`.
  - `ensure_*_kernel` bodies become `KernelSet::ensure`.
  - Nine non-canonical test keys are renamed.
  - The `repr_capable_kernel` field goes; S1's field partition is told (§12).
- **Signal:** the GUI test asserts `registered_kernel_names() == ["occt", "openvdb"]` and `default_kernel_name() == Some("occt")`. BT-ι4 is green.
- **Ratchet:** `kernel-default-sentinel | ^[^/]*(DEFAULT_KERNEL_NAME|__reify_eval_default_kernel|kernel_id_for_registry_name) | crates/reify-eval/src/**/*.rs | 0 | ι₃` (it covers `engine_build/tests.rs`).

### ι₄ (#8993) — `dispatcher::select` for ops, over the set's own descriptors (user-visible)
- **Keys:** none.
- **Files:** `crates/reify-eval/src/{dispatcher,engine_build,kernel_set,kernel_registry,engine_constraints,lib}.rs`, `crates/reify-cli/tests/harness_cli/cli_build_boolean_stl.rs` (new), `crates/reify-cli/tests/harness_cli.rs` (its `mod` line), `tests/infra/adoption-ratchet.manifest`.
- **Deps:** ι₃.
- **Work:**
  - The three `dispatch(` calls become `select(KernelRequest::Op{..})`.
  - The six build-routing `collect_registry()`/`registry()` reads are replaced by the set's descriptors (R1).
  - The BRep-fallback re-dispatch, the zero-conversion default fallback, the "target kernel not present" error and the None-arm sentinel move into `select`; the pragma-unsatisfiable check stays a warning.
  - ι₄ owns `crates/reify-cli/tests/harness_cli/cli_build_boolean_stl.rs`; #6104 depends on ι₄ and extends it (L15). The test embeds its own `.ri` source and reads no `tests/prd-gate/fixtures` file, so `_RUST_COUPLED_RI_FIXTURES` (a verify-pipeline file) is unchanged.
  - Why this is not #6104's rejected option (a): `select` plans over the descriptors of the kernels the set holds, so a plan never names an absent kernel and there is no blind fallback.
- **Signal:** BT-ι1 and BT-ι2 are green: `kernel_interface_boolean_stl_export.ri` exits 0, prints no internal error, and its STL spans x in [-5, 10] mm.
- **Ratchet:** `kernel-global-registry-routing | ^[^/]*(collect_registry[(][)]|kernel_registry::registry[(][)]) | crates/reify-eval/src/engine_build.rs | 0 | ι₄` (the three comment lines that mention it today do not count, by the `^[^/]*` prefix).

### ι₅ (#8995) — Handle-served requests go to the handle's owner (INV-GEO-2 consumer clause)
- **Keys:** none. **Subsumes #6137** (L14). **INV:** INV-GEO-2 (adds "a handle is served only by its owning kernel; absence is a coded Error"; pointers to `crates/reify-eval/tests/manifold_cross_kernel_real.rs` and #8391's B14 fixture; the leaf id is recorded on #8821).
- **Files:** `crates/reify-eval/src/{engine_build,engine_constraints,engine_eval,geometry_ops,realize_solid_sdf,morph_stage_b,selector_vocabulary_v2,shell_extract_compute,dispatcher,kernel_set}.rs`, `crates/reify-eval/src/kernel_owner.rs` (from #6308; absorbed into `select` and deleted), `crates/reify-eval/tests/manifold_cross_kernel_real.rs`, `docs/invariants.md`.
- **Deps:** ι₄, #8391.
- **Work:**
  - Re-express #8391's owner check and #6308's `resolve_owning_kernel_name` / `export_kernel_for_owners` through `select(OnHandle{owner})`, reusing `QueryKernelMismatch` / `QueryKernelUnserved` (no new code; INV-SF-6).
  - Keep the constructs #8391's delivered checks grep for: `.with_code(DiagnosticCode::QueryKernelMismatch)` and `OwnerMismatch`.
  - Batches partition by owner or report `MixedOwners`.
  - The three seed-cross re-executions route through `select(Op)`.
  - `default_query_kernel` goes.
  - The nine minted `KernelId::Occt` tags become the serving selection's kernel.
  - #6137's content: the union exported under `with_registered_kernels` is the Manifold-owned body, and a Mesh-only owner asked for `make_compound` yields a coded Error.
  - `distance_between_placed` (#6366 part (1)): a single owner serves the query, and mixed owners give `MixedOwners` → a coded Error. #6366 keeps its parts (2) and (3); the decompose session amends #6366 and adds the edge #6366 → ι₅.
- **Signal:** BT-ι3 is green. A test over two bodies with different owners asks for `distance_between` and gets a coded Error, not an OCCT answer. `git grep -c 'default_query_kernel' crates/reify-eval/src` = 0. `git ls-files crates/reify-eval/src/kernel_owner.rs` is empty. #8391's five delivered checks still hold.

### ι₆ (#8998) — CapabilityDescriptor declares queries, exports and projections; the hard-coded arms become rows
- **Keys:** fk-80c2f82c17d6.
- **Files:** `crates/reify-ir/src/geometry/capability.rs`, `crates/reify-kernel-{occt,manifold,fidget,openvdb,gmsh}/src/register.rs`, `crates/reify-eval/src/{dispatcher,engine_build,realization_content,realize_solid_sdf,engine_admin,engine_constraints,kernel_registry,measure_min_feature,measure_min_wall}.rs`, `crates/reify-kernel-conformance/tests/descriptor_accessor_parity.rs` (new; reify-kernel-conformance is in the nextest `occt` group, so no registration is needed), `tests/infra/adoption-ratchet.manifest`.
- **Deps:** ι₅, ε₄, ι₂.
- **Work:**
  - Add `queries: Vec<(QueryKind, ReprKind)>`, `exports: Vec<(ExportFormat, ReprKind)>` and `projections: Vec<(ReprKind, ProjectionKind)>`. Keep `Operation::Convert{from}`.
  - The seven hard-coded lookups become `select(Convert|Project)`.
  - `kernel_registry::openvdb_kernel_name` and its 14 call sites go (realization_content, realize_solid_sdf, measure_min_feature, measure_min_wall, engine_constraints).
  - realization_content's densify keys on the node's owner.
  - The registry test asserts both directions over every registration: each row implies its accessor is `Some`, and each `Some` accessor implies at least one row (L8).
  - gmsh's `register.rs` rows are born-then-deleted under #8288 (L17).
- **Signal:**
  - `descriptor_accessor_parity` is green. Seeding OpenVDB's descriptor with `(Convert{Mesh}, VolumeMesh)` turns it red.
  - A dispatcher unit test selects a mock declared under a non-gmsh `KernelId` for `Convert{Mesh→VolumeMesh}`.
  - `reify build examples/multi_kernel/voxel_to_mesh.ri` shows no regression.
- **Ratchet:** `kernel-hardcoded-arm | ^[^/]*(openvdb_kernel_name[(]|KernelId::(Gmsh|OpenVdb)[.]as_registry_name) | crates/reify-eval/src/**/*.rs | 0 | ι₆`.

### ω (#9000) — PRD close
- **Deps:** every leaf above.
- **Work:** the overlay's decompose-close stamp: Status `SHIPPED` with the landed leaf ids, the AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED map, and the same header on the capability manifest.
- **Signal:** the committed header.

## 9. Out of scope; follow-up candidates (not filed)

- **Out of scope:** the S11-owned OCCT internals (ShapeStore, op lowering, `call<R>`, history buffers); S13's selector module moves; S1's constructor; S2's per-template walk; S12's Diagnostic shape; meshing-service's gmsh sequence and worker; prose re-homing (S23).
- **Follow-up candidates:**
  - S11: return a cxx shared enum from `face_surface_kind`/`edge_curve_kind` and correct the "cxx cannot share enums" doc (L7).
  - A MockGeometryKernel `ReprPolicy` mirror, or lifting ζ's table into reify-ir, if a consumer of a mock's `BRepKind` ever appears (L4; none exists).
  - Descriptor rows for queries, exports and projections derived from the accessors through associated consts (L8 option B).
  - A `--verbose` "served by" line from a per-build selection record (L8; S1's BuildResult).
  - Whether the materialized-annotation overlay has a consumer coming (L11).
  - Typed SI for the eval-side `required_length_*` return types (beyond GeometryOp).
  - `faces_by_surface_kind`'s unknown-kind warning prints six times for one cell in `reify eval` (observed on the oracle fixture; diagnostic replay, S4/S12 territory).
  - Turning gmsh's inherent `mesh_to_volume` / `refine_volume_with_size_field` into `&ValidatedMesh` (deleting their three hand-rolled IndexValid copies); this is subsumed by meshing-service β/ζ, which replace them.

## 10. Open questions (tactical, decided at implementation)

- Exact sub-trait and accessor names, and the macro syntax for the holder's `Option<Box<dyn>>` arm.
- Whether `VolumeMeshing` splits into production versus store (two mocks implement only `volume_mesh`).
- Whether `QueryReply` lives in `geometry/query.rs` or a sibling `query_reply.rs`.
- Whether `Real` replies carry `reply_dimension()` from reify-ir (recommended: one table) or keep per-site dimensions.
- Whether `from_metres` returns `Option` (recommended) or panics.
- Whether HalfSpace keeps six scalar fields (recommended) or becomes `point: [Length;3], normal: [f64;3]`.
- Whether `value/compare.rs` and `value/display.rs` carve after measuring.
- Whether the `kernel_pragma` unknown ident becomes `None` (recommended) or `KernelPragma::Unknown(String)`.

## 11. Finding-key coverage and registry text

| key | disposition |
|---|---|
| fk-047cb467bd07 | ε₅ (high) |
| fk-ef5f5da8f729 | filed #8533 (in progress; not re-filed) |
| fk-a01d863c73ba | ζ₂ |
| fk-a60cf0fbe772 (p10, `language-pipeline|crates/reify-ir/src/geometry.rs::mesh|h10`) | ζ₂ |
| fk-1d857a1e9750 | θ₄ |
| fk-fdb4e221350a | η₆ |
| fk-59c46755d24c | η₃ (high) |
| fk-0375628ba585 | η₃ |
| fk-030602c2fd62 | η₃ (reassigned from S11) |
| fk-66c1d2fe9ff9 | α₁ |
| fk-80c2f82c17d6 | ι₆ |
| fk-e6a266a72f41 | δ₂ |
| fk-107e28de9029 | β₁ (R5: removed) |
| fk-92d313d2986f | stamped onto meshing-service β #8285 (R4) |
| fk-a0ce1853f2da | ζ₂ |
| fk-57ffd6bd2af2 | ι₁ |
| fk-08aa3fff3378 | θ₃ |
| fk-7b093573458b | α₅ |
| fk-448317051ff9 | γ |
| fk-65bb1ffb5d60 | filed #8561 (referenced; γ exception row) |
| fk-10ec3d2a1b76 | filed #8529 (in progress) |
| fk-0f933e75bf7d | β₁ (subsumes filed #8532) |
| fk-7bfef234d79e | δ₇ |
| fk-c497935d4b8a (S14's key, not S10's) | β₁, through the subsumed S14 κ #8632 (L13) |

**INV-GEO-4, corrected in the landing commit** (pre-#8821 five-column form, pointers inline): "test (`assert_kernel_contract!` in reify-test-support: the `real;` arm is instantiated for `OcctKernelHandle` and runs in the merge gate (test:crates/reify-kernel-occt/src/lib.rs); the `stub;` arm under `not(has_occt)` and the gmsh/OpenVDB `assert_stub_kernel_errors!` uses are compiled by no gate step and are removed by S10 β₁; Fidget and Manifold carry hand-written Send+Sync tests only)" | `partial: enforced(test) for OCCT, proposed for every other kernel (S10 δ₂–δ₄)`.

**INV-GEO-4, after δ₄:** "Every GeometryKernel implementation (real kernel and strict test mock) satisfies one shared contract suite: a representation-neutral core (Send+Sync, a dangling handle is never accepted, query_many length and agreement, extract_* idempotence) on every kernel, strict invalid-handle taxonomy where a path is supported, and a B-rep extension for kernels with topology" | test (test:crates/reify-kernel-occt/src/lib.rs; test:crates/reify-kernel-fidget/tests/kernel_contract.rs; test:crates/reify-kernel-manifold/tests/kernel_contract.rs; test:crates/reify-kernel-openvdb/tests/kernel_contract.rs; test:crates/reify-kernel-gmsh/tests/kernel_contract.rs; test:crates/reify-test-support/tests/mock_kernel_contract.rs) | enforced(test) | kernel-interface-typed-seam.

**INV-GEO-5 (new; partial at ε₂, enforced at ε₅):** "A delegating GeometryKernel wrapper forwards every capability: an omitted accessor is a compile error, and a delegating impl forwards every accessor to the field it forwards its core methods to (by `forward_kernel_capabilities!` or by hand); a masked accessor needs a counted, annotated reason" | type+lint (required accessors with no default bodies; `forward_kernel_capabilities!`; test:crates/reify-geometry/tests/kernel_wrappers_forward_every_capability.rs; test:crates/reify-geometry/tests/forwards_every_kernel_method.rs) | enforced(type+lint) | kernel-interface-typed-seam.

**INV-GEO-1 mechanism cell, after ζ₂ (ζ₄ closes its MarchingCubes clause):** "type+test (through the `GeometryKernel` seam: the mesh-consuming capability methods `ingest_mesh`, `ingest_mesh_at_resolution`, `register_mesh_handle`, `mesh_surface_to_volume`, `mesh_surface_to_volume_attributed` take `&ValidatedMesh`, minted only by `Mesh::into_validated(tol, posture)`, in production only at reify-eval's front door `admit_mesh` (tests mint through `reify_test_support::validated`); handing a kernel an unchecked Mesh is a compile error; mint sites pinned by adoption-ratchet row `mesh-mint-outside-front-door`; Finite and IndexValid are never waived; `REIFY_MESH_CONTRACT=warn` waives only NonDegenerate/Closed/ConsistentWinding and the waiver travels in the witness; `measure_mesh_deviation` takes the kernel's own tessellation as `&Mesh` by ruling R3; open: MarchingCubes output is exempt at the handoff until ζ₄, and gmsh's inherent `mesh_to_volume` / `refine_volume_with_size_field` take `&Mesh` from reify-solver-elastic outside the trait until meshing-service β/ζ replace them)".

**Owner map for S24 ξ #8821:** INV-GEO-1 → ζ₂ (ζ₄ amends its coverage clause); INV-GEO-2 → ι₅; INV-GEO-3 → **S11 ε #8682** (not S10; ξ's table needs this correction); INV-GEO-4 → δ₄; INV-GEO-5 → ε₂ then ε₅.

## 12. Cross-reference lines (for the lead)

- **S14 κ #8632.** Under R5 the gmsh stub (the only producer of `BackendUnavailable` and of `STUB_UNAVAILABLE_MARKER`) is deleted by S10 β₁, which also deletes the substring routing. **β₁ subsumes κ** (L13). The decompose session cancels κ with a reason naming β₁, moves its key fk-c497935d4b8a onto β₁ and rewires any dependent. The lead amends S14's FILED row and the program doc's S14 Q2 → (A) ruling line. κ's "empty `GMSH_LIB_DIR` compiles the stub arm" signal could not have worked as worded anyway: `find_dir_with_override` takes a non-empty override on trust, and an empty one falls through.
- **S11 θ #8672.** `stubs.rs` is deleted by S10 β₁; θ skips its stub-doc hunk if the file is gone. Trait-impl blocks in handle.rs are S10's edit after θ (R1).
- **S11 / program G4 table.** Add the row "OcctKernelHandle GeometryKernel/sub-trait impl blocks | S10 (R1) | S11 keeps the inherent surface". Add the row "KernelSet (reify-eval) | S10 | S1 constructor".
- **S1.** S10 ι₃ defines `KernelSet` without changing any Engine constructor signature. ι₄ takes the build-routing `collect_registry()`/`registry()` reads (part of fk-4db43199eda2). S1 keeps the link anchors, its single constructor (consuming `KernelSet`) and the `test_registry_override` deletion. ι₃ deletes the `repr_capable_kernel` field and changes `with_test_kernels_and_registry`'s key type (11 files call it, by `git grep -l`).
- **S24 ξ #8821.** The owner map is in §11. INV-GEO-3's owner is S11 ε #8682, not S10. INV-GEO-5 is a new row, made partial by ε₂ and completed by ε₅.
- **S6.** No S10 leaf consumes JointKind or RelationKind (premise correction 6); the program's "S10 consumes β 8829 / γ 8830" line has no consumer site. The reify-ir `lib.rs` overlap is lock-level only.
- **S3 / INV-ARCH-1.** α₁ lowers the reify-ir row to `[[completeness, ranked], [expr, value]]`; `{expr, value}` is a recorded residual (L10).
- **S5.** γ's strata table classifies every workspace crate. S5's new analysis crate must declare its stratum in its own diff (the unclassified-crate rule turns it red otherwise). Classify the new analysis crate in `scripts/crate-strata.json` in its own diff, and lower γ's `compute-contract → solver-elastic` exception row in the same diff that drops the edge.
- **meshing-service (#8284, #8285, #8288, #8289).** Notes are appended at decomposition:
  - #8284's defaulted `tessellate_with_topology` is moved into the `TopologyTessellation` sub-trait by ε₁, and its delivered check is widened to `crates/reify-ir/src` (L9);
  - #8285 carries fk-92d313d2986f (R4), and its `GeometrySource::Surface` should carry a `ValidatedMesh` after ζ₂; `STUB_UNAVAILABLE_MARKER` is already gone when it lands (β₁);
  - #8288 deletes the `VolumeMeshing` sub-trait if ε₄ landed;
  - `GMSH_AVAILABLE` may already be gone by β₃.
- **#6104.** It depends on ι₄ (the decompose session adds the edge; L15). ι₄ owns `cli_build_boolean_stl.rs`, and #6104 extends it. ι₄ is not #6104's rejected fallback, because `select` plans only over the kernels the engine holds. A note is appended.
- **implicit-solids (#8391, #8396, #8399).** ι₅ depends on γ #8391 and reuses its codes and constructs (L14). θ₃ deletes the kernel length tripwire that `implicit-solids.md` §7.3–7.4 cites as live, so the next implicit-solids pass must not re-arm it, and its `ImplicitSolid` op adopts `Length` and the θ₄ justification guard.
- **#6366.** It is split (L14): ι₅ takes part (1), `distance_between_placed`. #6366 keeps (2) the declarative `: Output` export and (3) `resolve_export_body_color`, **depends on ι₅**, and builds on `select(OnHandle)` instead of #6308's helpers. Implicit-solids υ #8399, which depends on #6366, therefore waits for ι₅.
- **#6137.** Subsumed by ι₅ plus #6308 (cancelled at decomposition after its content is ported; L14).
- **S11 (cross-reference "MockGeometryKernel mirrors ReprPolicy").** Not built: no reify-eval code reads `BRepKind` (premise correction 13, G1). It is recorded as a follow-up candidate.
- **meshing-service ζ #8289.** Lower γ's `solver-elastic → kernel-gmsh` exception row in the same diff that drops the edge (note appended).
- **S25 / S26.** After β₃, `get_kernel_status` (`gui/src-tauri/src/kernel_status.rs`) always reports OCCT available. Whether the command and its TS consumer stay is theirs to decide.
- **S1 (anchors).** β₃ removes `cfg(has_openvdb)` from the link-anchor lines in `crates/reify-cli/src/main.rs` and `gui/src-tauri/src/lib.rs`, and keeps the anchor lines themselves.
- **#8561.** Lower γ's `kernel-occt → mesh-morph` exception row in the same diff (note appended).
