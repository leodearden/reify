# Capability Manifest — kernel-seam-contracts

Mechanizes G3+G6 per leaf (`gates.md` → *Capability Manifest*). This PRD is **near-pure Rust wiring**:
its only `.ri` fixture uses existing grammar, so the executable `.ri` D3 substrate-verify workflow is
**N/A** (`procedural_prd_d3_verify_workflow_is_ri_only`) and the binding-of-record is **wired-on-main
grep evidence** (anti-orphan). Field-population (`Value::Field`) and the FEA/spline/eigensolver
**numeric floors are N/A across every leaf** — the sole numeric axis is the weld tolerance (a
correctness quotient, not an accuracy bound), pinned to Manifold's existing bit-exact weld (§13 Q2).

Evidence forms used: **wired** (production call site the leaf must establish — the anti-orphan grep
target once landed), **grammar** (fixture parses under existing grammar), **behavior** (a real
behavioral observable, not synthetic-input unit test), **verify** (test wired into the verify pipeline,
not an orphaned/test-only-declared symbol).

| Leaf | INV | Capability asserted | Evidence binding | Verdict |
|---|---|---|---|---|
| **α** | GEO-1 | `Mesh::validate()`/`weldedness()` + `GeometryError::MeshContractViolation` exist and encode obligations-vs-capabilities | **wired (integration-gated):** consumers are β,γ,ν,#4876 in this batch (all depend on α); direct = `validate()` rejects the 4336 reversed fixture, **accepts** unwelded OCCT input. Grammar N/A (reify-ir Rust). | PASS — consumer leaves named & depend on α |
| **β** | GEO-1 | Handoff executor emits the structured diagnostic to the user surface | **wired:** post-landing grep `validate(` in `engine_build.rs` between tessellate(@6936) and ingest_mesh(@6963); **anti-inversion:** the `MeshContractViolation` reaches `EvalResult`/CLI diagnostics, not swallowed at the kernel. **grammar:** fixture `.ri` (Boolean→`Mesh`) = existing grammar, `grammar_confirmed=true`. | PASS |
| **γ** | GEO-1 | Manifold ingest returns the structured error with counts | **wired:** grep `validate(` in `kernel.rs` before `from_mesh_f64`(@270); observable through existing `manifold_cross_kernel_real.rs`. | PASS |
| **δ** | GEO-1 | Enforce-default ships with a real break-glass knob | **wired+verify:** `REIFY_MESH_CONTRACT` read on the enforcement path; locking test pins enforce-default AND `=warn` downgrade; corpus sweep output attached to the leaf. Flips registry row. | PASS (rollout leaf; sweep enumerates the tail) |
| **ε** | GEO-2 | Conformance suite runs REAL kernel output, wired into verify | **verify:** new `reify-kernel-conformance` is a **workspace member** run by the verify pipeline (not an orphan crate); fixtures are real OCCT/gmsh tessellations, not mocks. | PASS |
| **ζ** | GEO-2 | Handle-stability observed on real kernels | **behavior+verify:** repeated `extract_*` on real parents → stable ids; test in the verify-run crate. | PASS |
| **η** | GEO-2 | Manifold `extract_edges` memoized; stability arm green | **behavior:** the manifold-edges stability arm is **red on revert**, green with the `extracted_faces`-mirrored cache. | PASS |
| **θ** | GEO-4 | One suite source asserts the same taxonomy under both cfgs | **verify:** suite instantiated under `has_occt` AND `not(has_occt)`, both run in verify; a taxonomy divergence fails. | PASS |
| **ι** | GEO-4 | OCCT stub adopts the shared suite (bespoke deleted) | **wired (anti-orphan of adoption):** grep confirms `stubs.rs:551-791` bespoke `mod tests` + `assert_stub_message` are **removed** and replaced by the shared instantiation — adoption total, old shape unrepresentable. | PASS |
| **κ** | GEO-3 | `parent_handle` classified; OwnerBody correct after restore | **behavior:** `owner_body_survives_warm_start` is **red on current main** (stale/wrong OwnerBody), green after; exhaustive destructure makes a new occt field a **compile error**. | PASS (fixes a live latent bug) |
| **λ** | GEO-3 | Warm-start failures visible in production | **wired:** `tracing::warn` on the restore-failure path (not `eprintln!`); production (non-`cfg(test)`) accessor grep-confirmed. | PASS |
| **μ2** | GEO-3 | Manifold side tables all classified | **behavior:** exhaustive destructure of `{shapes,sub_shapes,extracted_faces}`; unclassified field = compile/test failure. | PASS |
| **μ3** | GEO-3 | gmsh volume-mesh store classified; flips registry | **behavior:** exhaustive destructure of `VolumeMeshStore{next_id,meshes}`; unclassified field fails. | PASS |
| **ν** | GEO-1 | Validator fires on the **real** 4876 OCCT tessellation | **behavior+grammar:** `validate()`/`weldedness()` on the real fixture tessellation reports the concrete witness (counts); fixture `.ri` = existing grammar. Guards the G6 "does the mechanism fire on real, not synthetic, input" discipline. | PASS |
| **#4876** | GEO-1 | Preflight converts SIGSEGV → diagnostic + graceful degrade | **behavior:** `fea_face_selector_bc_e2e` boundary-demand test **un-ignored** → degrade + visible diagnostic, no SIGSEGV (was `#[ignore]`'d on this exact crash). | PASS (fail-closed immediately; the §4 exception) |
| **ξ** | GEO-1 | Attributed producer works on OCCT tessellations, attribution preserved | **behavior:** §5 gmsh-attributed conformance arm **un-ignored** → real attributed volume mesh; per-node attribution survives the vertex-merge correspondence map. | PASS |

**No FAIL bindings** → batch is clear to queue. The one G6-sensitive claim ("attribution preserved
across welding", ξ) is a **correctness** property verified by the conformance re-validate + attribution
check, not a numeric bound, so no floor applies. The weld-tolerance quotient (α/§13 Q2) is pinned to
the existing bit-exact weld — no new numeric premise is introduced.
