# Kernel-Seam Contracts and Conformance

Status: contract (B+H full shape). Authored 2026-07-06 in interactive `/prd` session as part of the
bug-hotspot program (`docs/notes/bug-hotspot-survey-2026-07-05.md` §H3). Establishes/strengthens
**INV-GEO-1..4** in `docs/invariants.md`. Owner PRD named there for those four IDs.

## §0 — Purpose, consumers, and scope

The `Mesh` interchange type (`crates/reify-ir/src/geometry.rs:2483`, doc: "Tessellated mesh for
visualization") is a plain `{vertices, indices, normals}` struct with **no invariants and no
validator**. The only weld/closed/orientable enforcement in the codebase lives *inside Manifold's
ingest* (the 4329 fix, `crates/reify-kernel-manifold/src/kernel.rs:228-270`), so it protected exactly
one consumer. That is the structural generator of the serially-discovered handoff holes:
4329 (unwelded verts) → 4336 (REVERSED-face winding) → **live 4876** (gmsh attributed producer
SIGSEGVs on unwelded OCCT output) — the same defect re-instanced at each consumer the fix never
touched. Meanwhile 59 `impl GeometryKernel` exist (5 real, ~45 bespoke mocks) against exactly two
real-output cross-kernel test files; handle identity is per-kernel folklore; and warm-start carries
a live latent bug (`parent_handle` neither persisted nor cleared across restore).

**The fix shape (survey cross-cutting theme 3): a seam-owned contract type + validator + conformance
suite over real outputs, replacing "contract held in one consumer".**

**Consumers (G1).** The contract has real cross-kernel Mesh-routing consumers today:
- Any Boolean of OCCT BRep solids **demanded as `Mesh`** (Stl/Obj sink, viewport) routes OCCT
  tessellation → Manifold ingest (existing e2e `crates/reify-eval/tests/manifold_cross_kernel_real.rs`).
- FEA **volume meshing** consumes tessellated surfaces via the gmsh producers
  (`mesh_surface_to_volume{,_attributed}`) — the live 4876 crash path.
- Leo dogfoods `prj/printer_v01`.

The validator (§2/§3) is a **producer** whose named consumers are the three seam wirings in this PRD
(§4) plus the conformance suite (§5); the C-as-integration-gate DAG (§12) makes every substrate leaf
answer to a consumer leaf that carries the end-to-end signal.

**In scope:** INV-GEO-1 (MeshContract validator + fail-closed rollout + the 4876 preflight & real
fix), INV-GEO-2 (kernel-pair conformance suite + handle-stability + Manifold `extract_edges`
memoization), INV-GEO-3 (warm-start drift guards + the `parent_handle` bug fix), INV-GEO-4 (one
shared stub/real contract suite).

**Out of scope (owned elsewhere — see §10):** the `KernelHandle` re-key of attribute tables (#4351,
engine-build-hardening PRD); splitting `geometry_ops.rs`/`engine_build.rs` (god-file-decomposition
PRD); the unified builtin-name metadata table (survey C6); the KGQ capability-gate finish (survey
C7). This PRD makes the seam *correct*, not *smaller*.

## §1 — Preconditions and invariant linkage

All code anchors below were re-verified against `main` (HEAD `4d696e6`, 2026-07-06); line numbers are
current. This PRD is near-pure Rust wiring — its single `.ri` fixture (§4 signal, §11) uses **existing
grammar** (Boolean → `Mesh` demand), so `grammar_confirmed=true` for every leaf and the executable
`.ri` D3 substrate-verify workflow is **N/A**; the capability manifest binds wired-on-main grep
evidence instead (`kernel-seam-contracts.capability-manifest.md`; rationale:
`procedural_prd_d3_verify_workflow_is_ri_only`).

Registry rows this PRD owns (`docs/invariants.md`): INV-GEO-1 (type+test, fail-closed with
`REIFY_MESH_CONTRACT` break-glass), INV-GEO-2 (type+test; **co-owned** with engine-build-hardening —
this PRD lands the conformance-property-test half, #4351 lands the keyed-table half), INV-GEO-3
(type+test), INV-GEO-4 (test). Per **INV-META-1**, every leaf cites its INV-id and its enforcement
mechanism is part of done-criteria, and the registry `proposed → enforced(...)` flip lands in the
same change that lands the enforcement (§12 names which leaf flips which row).

## §2 — The MeshContract (INV-GEO-1)

The contract distinguishes **producer obligations** (every kernel that emits a `Mesh` must satisfy
these) from **consumer-declared capabilities** (properties a *specific* consumer additionally
requires, which the producer is not obliged to provide).

**Producer obligations — a conforming `Mesh`:**
1. **Finite** — all vertex coordinates and normals are finite (no NaN/Inf).
2. **Index-valid** — every triangle index is in `0..vertices.len()`.
3. **Non-degenerate** — no triangle has a repeated index or zero area beyond tolerance.
4. **Closed** — on the **position-welded quotient topology**, every directed edge has its reverse
   exactly once (no open boundary).
5. **Consistently wound** — same quotient: no directed edge appears twice in the same direction
   (orientable, coherent winding).

**The critical content fact (why obligations 4–5 are checked on the *quotient*, not raw indices):**
OCCT `tessellate_shape` emits **per-face vertex blocks — unwelded by design**
(`crates/reify-kernel-occt/cpp/occt_wrapper.cpp:5847`, per-`TopAbs_FACE` loop with per-face
`vertex_offset`). A naive "output indices must be welded" obligation would fail **100 % of OCCT
output on day one**. Therefore the validator **position-welds internally** (tolerance quotient) before
checking closed/wound, exactly as the lifted checker already does (`weld_vertices` then the
directed-edge invariant). **Weldedness of the raw indices is a *capability axis*, not an obligation.**

**Consumer-declared capabilities:**
- `requires_welded` — the consumer needs already-welded indices (shared vertices), not per-face
  blocks. Manifold ingest declares this and satisfies it **defensively itself** (bit-exact weld,
  `kernel.rs:229-268`). The **gmsh attributed producer** declares it and **cannot** repair without
  destroying attribution (`mesh_boundary.rs:219-227` forbids vertex-merging repair) — which is the
  4876 SIGSEGV.

The contract is documented on the trait surface — real rustdoc on `GeometryKernel::tessellate`
(`geometry.rs:3488`, currently the single line "Tessellate a handle into a mesh.") and on
`ingest_mesh` (`geometry.rs:3650`, doc 3610-3649) — stating producer obligations and naming
weldedness as a consumer capability, so the "OCCT unwelded is legal" fact lives at the seam, not in
tribal memory.

## §3 — Validator placement and structured error (INV-GEO-1)

**`validate()` lives in `reify-ir` next to `Mesh`** (`geometry.rs`), not in any kernel — it is the
seam type, and reify-ir has no kernel deps (the checks are pure combinatorics + coordinate finiteness
on a `Mesh`, so they are testable without any real kernel). Shape:

```rust
pub struct ValidatedMesh(Mesh);                 // proof-carrying newtype; only validate() mints it
pub struct WeldednessReport { pub raw_welded: bool, pub weld_merged_verts: usize }

impl Mesh {
    /// Producer obligations (finite, index-valid, non-degenerate, closed, consistently wound)
    /// checked on the position-welded quotient. Weldedness of raw indices is reported, not gated.
    pub fn validate(&self, tol: f64) -> Result<ValidatedMesh, MeshContractViolation>;
    pub fn weldedness(&self, tol: f64) -> WeldednessReport;   // the consumer-capability axis
}
```

**Lift** the weld+winding+closedness checker already hand-rolled at
`crates/reify-kernel-occt/tests/tessellation_winding_integration.rs` (`weld_vertices` + directed-edge
invariant + outward-normal check) into `reify-ir` as the body of `validate()`. The OCCT test then
consumes the lifted function instead of its private copy (kills the duplicate).

**Structured error** replaces the string `OperationFailed` for mesh failures. Add to
`GeometryError` (`geometry.rs:3100`, currently only `OperationFailed(String)` @3104):

```rust
MeshContractViolation {
    kernel: &'static str,          // "occt" | "manifold" | "gmsh"
    invariant: MeshInvariant,      // Finite | IndexValid | NonDegenerate | Closed | ConsistentWinding
    counts: MeshViolationCounts,   // e.g. { open_edges, reversed_edges, nan_verts, oob_indices }
    witness: MeshWitness,          // one concrete offending edge/triangle/vertex for the diagnostic
}
```

This is what turns "string `NotManifold` three layers downstream" and "SIGSEGV" into a diagnostic
naming kernel + invariant + counts.

## §4 — Enforcement rollout (INV-GEO-1, fail-closed end-state)

**Leo-ratified posture (decision #5; `docs/invariants.md` header):** *fail-closed is the end-state
everywhere.* Per-invariant rollout: written spec → **one-shot warn-mode corpus sweep** (test corpus +
`examples/` + `prj/`) to batch-enumerate violations → fix bulk producers → **flip to enforce** for the
tail. Break-glass env knob **`REIFY_MESH_CONTRACT=warn`** mirrors the main-gate ENFORCE/BYPASS pattern
(default = enforce post-flip).

Three wiring sites:
1. **Handoff executor** — `crates/reify-eval/src/engine_build.rs`, between `src.tessellate(...)` (@6936)
   and `.ingest_mesh(&mesh)` (@6963), which today validate **nothing**. Warn-default during rollout.
2. **Manifold ingest** — `kernel.rs` before `from_mesh_f64` (@270): validate for early, structured
   diagnostics (the existing defensive weld stays; validation adds provenance + counts).
3. **gmsh attributed producer** — the **immediate-enforce exception**: flipped fail-closed at once,
   **no sweep** (the alternative is a SIGSEGV). This is the 4876 preflight (§9).

Sites 1–2 flow through the warn-sweep→enforce rollout together (one leaf). Site 3 skips the sweep.

## §5 — Kernel-pair conformance suite (INV-GEO-2)

**New dedicated crate `crates/reify-kernel-conformance`** (tests-only; dev-deps: `reify-ir`,
`reify-kernel-occt`, `reify-kernel-gmsh`, `reify-kernel-manifold`, `reify-eval`). Chosen over
`reify-eval/tests/` (Leo, 2026-07-06) for clear seam ownership and isolation from `reify-eval`'s heavy
test compile and the concurrent god-file test-module churn.

**Matrix:** registered producers × consumer entry points (`ingest_mesh`, `mesh_surface_to_volume`,
`mesh_surface_to_volume_attributed` — trait decls `geometry.rs:3650/3779/3825`, impls
`kernel_real.rs:464/524`) running **real tessellated fixtures** — box, sphere, cylinder,
**boolean-with-`TopAbs_REVERSED`-faces**, fillet — through **produce → validate (§3) → consume →
re-validate**. cfg-gated `has_occt`/`has_gmsh` like the existing cross-kernel tests
(`manifold_cross_kernel_real.rs`).

**Handle-stability property tests** (would have caught 4262): repeated `extract_faces`/`extract_edges`
on the same parent return **stable ids** within a session, per real kernel. The **Manifold
`extract_edges` arm is red today** (un-memoized, §8) and becomes §8's acceptance test.

**The gmsh-attributed arm** starts `#[ignore = "blocked on #<ξ> — attribution-aware repair"]` and
becomes the acceptance test for the 4876 real fix (§9 ξ).

## §6 — Warm-start drift guards (INV-GEO-3)

**Live latent bug (fix, do not merely guard):** `OcctKernel::with_warm_state`
(`crates/reify-kernel-occt/src/lib.rs:4140`) clears the three `extracted_*` caches (@4188-4190) but
**neither persists nor clears `parent_handle`** (`lib.rs:517`) → stale/**wrong `OwnerBody`** answers
after restore. Fix: classify `parent_handle` explicitly (clear-on-restore, matching the `extracted_*`
caches) and add an `owner_body_survives_warm_start` round-trip test next to the existing
`warm_start_roundtrip_{single,multi}_shape` tests (`lib.rs:4750/4781`). Fix the genuinely-stale
`reprs`-field doc (`lib.rs:476-478`) that claims warm-start doesn't repopulate `reprs` (it now does,
@4180-4188).

**Drift guard (type):** rebuild `warm_state` (`lib.rs:4093-4137`) through an **exhaustive no-wildcard
pattern** — the `elastic_result.rs:617-642` precedent (exhaustive struct literal / `let Self { .. }`
with no `..` spread) — so a new per-handle field forces a compile-time persist/clear/rebuild decision.

**Production observability:** promote `last_warm_start_failures` (`lib.rs:520`) from `eprintln!`
(@4156) + a `#[cfg(all(test, has_occt))]` accessor (@4216) to `tracing::warn` + a production accessor.

**Per-kernel state inventory (type+test):** each kernel classifies **every** per-handle side table as
persist / clear / rebuild via an exhaustive destructure, so an unclassified new field fails:
- occt: `shapes`(474) `reprs`(479) `extracted_edges`(492) `extracted_faces`(496)
  `extracted_vertices`(506) `parent_handle`(517) — covered by the `warm_state` destructure above.
- manifold (`kernel.rs:87`): `shapes`(91) `sub_shapes`(97) `extracted_faces`(118).
- gmsh (`kernel_real.rs:59`): `volume_mesh_store`(60) → `VolumeMeshStore`(67) `{next_id, meshes}`.

## §7 — Shared stub/real contract suite (INV-GEO-4)

One macro/generic suite asserting the **shared observable contract**, instantiated for **real
(`has_occt`)** and **stub (`not(has_occt)`)** from a single source:
- **Error taxonomy per method** — `InvalidReference` vs `OperationFailed` vs `QueryFailed` are emitted
  by the same methods on stub and real.
- **`query_many` length invariant** — output length matches input length.
- **`extract_*` stability** — shared with §5's property axis.

**Extend/supersede** `assert_stub_kernel_errors!` (`crates/reify-test-support/src/kernel_assertions.rs:78`,
which already emits Send+Sync + boolean/query/export/tessellate error assertions). **Adopt it in the
OCCT stub**, deleting the bespoke `mod tests` assertions + local `assert_stub_message` helper
(`crates/reify-kernel-occt/src/stubs.rs:551-791`) — the proof of adoption is that the bespoke code is
*gone*, not merely shadowed.

## §8 — Manifold `extract_edges` memoization (INV-GEO-2)

`ManifoldKernel::extract_edges` (`kernel.rs:812`) is **un-memoized** — a dormant 4262 re-instance:
`extract_faces` (@767) got the `extracted_faces` cache (field @118) + `coalesce_coplanar_faces` (@791)
in the 4262 fix; edges did not. Currently masked in production by BRepOnly gating
(`crates/reify-eval/src/topology_selectors.rs:1897`, `ByRole → BRepOnly` @1916), so it surfaces only
under the conformance property test. Fix = mirror the `extracted_faces` cache for edges; acceptance =
§5's Manifold `extract_edges` stability arm goes green (demonstrably red on revert).

## §9 — The 4876 cluster (INV-GEO-1; attach to this milestone — Leo explicit)

Adopt existing task **#4876** (`deferred`, high) — do not duplicate. Decomposed into three leaves:

**ν — Characterize** the exact OCCT fixture tripping tetgen. Run the fixture's **real** OCCT
tessellation (named in #4876 / `crates/reify-eval/tests/fea_face_selector_bc_e2e.rs`) through the §3
validator + `weldedness()` axis; record the concrete witness (unwelded vertex count, open/non-
watertight edges). This proves the mechanism fires on the **real** input (G6 discipline), and is the
diagnostic bridge both downstream leaves consume.

**#4876 (preflight, near-term)** — Rust-side watertightness preflight in the attributed path
(`mesh_boundary.rs:210` / `repair.rs:71`): before entering gmsh, run the §3 weldedness/closedness
capability check; on non-watertight input return `Err(MeshContractViolation)` (**fail-closed
immediately — the §4 site-3 exception**) so the realization edge's existing honest-degradation falls
back to the plain producer with a visible diagnostic. Converts the SIGSEGV to a diagnostic. (Update
#4876's stale `metadata.files`: the cited `mesh_surface_to_volume_attributed.rs` does not exist →
`mesh_boundary.rs` + `repair.rs`.)

**ξ — Real fix: attribution-aware repair.** Thread a vertex-merge **correspondence map** through
`repair_surface_mesh` (`repair.rs:71`) so per-node attribution survives welding, replacing the
outright rejection at `mesh_boundary.rs:219-227`. The attributed producer then works on OCCT
tessellations; acceptance = §5's ignored gmsh-attributed conformance arm is un-ignored and green
(real attributed volume mesh, attribution preserved).

## §10 — Cross-PRD seams and ownership (G4)

| Seam | Owner | This PRD's posture |
|---|---|---|
| `KernelHandle` re-key of attribute tables (**#4351**) — the `(kernel,id)` half of INV-GEO-2 | **engine-build-hardening** PRD (concurrent) | Reference only; **do not file #4351 here.** This PRD lands only the conformance-property-test half (per-parent `extract_*` stability), which needs no re-key. Wire a real dep edge only if a leaf's table work comes to need it (none does today). |
| Evicting `geometry_ops.rs` / `engine_build.rs` **test modules** | **god-file-decomposition** PRD (concurrent) | Tolerate it landing first (wholesale test moves). This PRD's edits to `engine_build.rs` (§4 site 1) touch **production** code; if that PRD splits `engine_build.rs` into `engine_realize.rs`/`engine_tessellate.rs`, the §4-β leaf rebases the handoff-executor edit onto the new home. No hard edge; BRE handles the footprint. |
| Mesh contract in Manifold ingest (the 4329 fix) | This PRD (INV-GEO-1) | §4 site 2 generalizes it from one-consumer to seam-owned; the existing defensive weld stays. |

No new contested-ownership pair is introduced (the three in the overlay's breadcrumb map are
untouched).

## §11 — Boundary-test sketch (cross-crate; facing both ways) — G5/H

The seam is between **`reify-ir`** (the `Mesh` type + `validate()`) and the kernel crates
(`reify-kernel-occt/-manifold/-gmsh`) + the engine (`reify-eval`). Tests cross it from each side.

### 11.1 Producer-side (a kernel emits a Mesh; the seam validates it)
| Scenario | Precondition | Postcondition |
|---|---|---|
| **Reversed-winding rejected.** Feed the boolean-with-`TopAbs_REVERSED`-faces fixture through produce→validate. | Real OCCT tessellation of the 4336 case. | `validate()` → `MeshContractViolation { invariant: ConsistentWinding, counts.reversed_edges > 0, witness }`. |
| **Unwelded OCCT accepted.** Feed a plain OCCT box tessellation (per-face blocks) through validate. | Real OCCT output, unwelded. | `validate()` → `Ok(ValidatedMesh)`; `weldedness().raw_welded == false` (weldedness reported, **not** gated). |
| **Handle stability.** Repeated `extract_faces`/`extract_edges` on one parent. | Real kernel. | Identical id sequences across calls (Manifold edges: red pre-§8, green post). |

### 11.2 Consumer-side (a consumer requires a capability the producer may not provide)
| Scenario | Precondition | Postcondition |
|---|---|---|
| **Handoff diagnostic (signal a).** `.ri` Boolean of two OCCT boxes demanded as `Mesh`, with a deliberately-corrupted producer (injected reversed winding). | §4 site-1 wired, `REIFY_MESH_CONTRACT` enforce. | `reify`/e2e emits the structured `MeshContractViolation` naming kernel+invariant+counts — **not** string `NotManifold`, **not** a segfault. |
| **Manifold ingest diagnostic.** Contract-violating mesh into `ingest_mesh`. | §4 site-2 wired. | `Err(MeshContractViolation{ kernel:"manifold", .. })` with counts, replacing the generic downstream string; valid unwelded OCCT still ingests. |
| **gmsh degradation (signal b).** 4876 boundary demand on the real OCCT box surface. | §9 preflight landed. | Graceful degrade to the plain producer + visible diagnostic; **no SIGSEGV** (`fea_face_selector_bc_e2e` test un-ignored). |
| **gmsh real fix.** Same demand, attributed path, post-ξ. | §9 ξ landed. | Real attributed volume mesh; per-node attribution preserved; §5 gmsh-attributed arm green. |
| **Stub ≡ real taxonomy.** Same method calls under `has_occt` and `not(has_occt)`. | §7 suite. | Identical error variants from one source; bespoke stub assertions deleted. |
| **Warm-start round-trip.** Save→restore, then query OwnerBody. | §6. | Correct `OwnerBody` (red on current main); a new unclassified side-table field fails compile. |

## §12 — Decomposition DAG (per-leaf observable signal; task IDs assigned at decompose time)

Style **B (vertical slice) + H (contracts + two-way boundary tests)**. α is the only pure-substrate
leaf; it is **integration-gated** by β/γ/ν per the C-as-integration-gate escape (`gates.md` G2). Every
leaf cites its INV and the enforcement mechanism is in its done-criteria (INV-META-1).

**INV-GEO-1 — validator + wiring + rollout**
- **α** — `MeshContract`/`ValidatedMesh` + `validate()`/`weldedness()` + `GeometryError::MeshContractViolation` in `reify-ir`; lift the OCCT-test checker; rustdoc the contract on `tessellate`/`ingest_mesh`. *Signal:* integration-gated by β; direct coverage = `validate()` rejects the 4336 reversed-winding fixture with counts and **accepts** unwelded OCCT-style input. *Files:* `crates/reify-ir/src/geometry.rs`. *Deps:* none.
- **β** — wire `validate()` at the handoff executor (`engine_build.rs`), warn-default. *Signal (a):* `.ri` Boolean of two OCCT boxes as `Mesh` + corrupted producer → structured `MeshContractViolation` diagnostic via `reify`/e2e (not `NotManifold`, not segfault). *Files:* `crates/reify-eval/src/engine_build.rs`. *Deps:* α.
- **γ** — wire `validate()` in Manifold ingest before `from_mesh_f64`. *Signal:* contract-violating mesh → `Err(MeshContractViolation{kernel:"manifold",..})` with counts through `manifold_cross_kernel_real.rs`; valid unwelded OCCT still ingests. *Files:* `crates/reify-kernel-manifold/src/kernel.rs`. *Deps:* α.
- **δ** — INV-GEO-1 rollout: one-shot warn-sweep (corpus + `examples/` + `prj/`) → fix bulk producers → flip default to enforce for sites 1–2; add `REIFY_MESH_CONTRACT` knob + locking test. **Flips INV-GEO-1 → enforced** (gmsh site covered by §9 preflight). *Signal (c-adjacent):* verify green with enforce default across corpus; a known-violating fixture fails closed; `REIFY_MESH_CONTRACT=warn` downgrades to warning (locking test pins both). *Files:* `[]` (env-knob plumbing + swept producers — footprint unknown; BRE acquires). *Deps:* β, γ.

**INV-GEO-2 — conformance + handle stability + extract_edges**
- **ε** — new `crates/reify-kernel-conformance` crate + producer×consumer matrix over real fixtures (produce→validate→consume→re-validate); gmsh-attributed arm `#[ignore]` citing ξ. *Signal (c):* suite green in verify for all non-ignored kernel pairs; a contract-violating producer fails produce→validate. *Files:* `[]` (greenfield crate + workspace `Cargo.toml`). *Deps:* α.
- **ζ** — handle-stability property tests in the conformance crate (currently-stable arms: occt faces/edges, manifold faces). *Signal:* stable-id property tests green; an unstable kernel fails. *Files:* `[]` (in the ε crate). *Deps:* ε.
- **η** — Manifold `extract_edges` memoization (mirror the `extracted_faces` cache) + add the manifold-edges stability arm to ζ. **Flips INV-GEO-2 conformance-half → enforced** (keyed-table half tracked by #4351). *Signal:* the manifold `extract_edges` stability arm goes green (red on revert). *Files:* `crates/reify-kernel-manifold/src/kernel.rs`. *Deps:* ζ.

**INV-GEO-4 — shared stub/real suite**
- **θ** — shared macro/generic contract suite (error taxonomy, `query_many` length, `extract_*` stability), instantiated under `has_occt` and `not(has_occt)`; extend `assert_stub_kernel_errors!`. *Signal:* one suite source runs green under both cfgs; a stub/real taxonomy divergence fails it. *Files:* `crates/reify-test-support/src/kernel_assertions.rs`. *Deps:* none.
- **ι** — OCCT stub adopts the shared suite; delete bespoke `mod tests` assertions + `assert_stub_message`. **Flips INV-GEO-4 → enforced.** *Signal:* stub tests instantiate the shared suite; bespoke code deleted; verify green under `not(has_occt)`. *Files:* `crates/reify-kernel-occt/src/stubs.rs`. *Deps:* θ.

**INV-GEO-3 — warm-start drift guards**
- **κ** — `warm_state` exhaustive-destructure (occt inventory) + **fix `parent_handle`** + `owner_body_survives_warm_start` round-trip test + fix stale `reprs` doc. **Flips INV-GEO-3 occt-portion → enforced.** *Signal:* `owner_body_survives_warm_start` returns the correct body after restore (red on current main); a new unclassified occt side-table field fails compile. *Files:* `crates/reify-kernel-occt/src/lib.rs`. *Deps:* none.
- **λ** — promote `last_warm_start_failures` to `tracing::warn` + production accessor. *Signal:* injected restore failure emits a `tracing::warn` (captured via test subscriber) + the production accessor returns the count in a non-test build. *Files:* `crates/reify-kernel-occt/src/lib.rs`. *Deps:* κ.
- **μ2** — manifold state-inventory drift guard (exhaustive destructure classifying `shapes`/`sub_shapes`/`extracted_faces`). *Signal:* completeness test classifies every manifold side table; an unclassified field fails. *Files:* `crates/reify-kernel-manifold/src/kernel.rs`. *Deps:* none.
- **μ3** — gmsh state-inventory drift guard (`volume_mesh_store`/`VolumeMeshStore`). **Flips INV-GEO-3 → enforced** once κ+μ2+μ3 land. *Signal:* completeness test classifies the gmsh volume-mesh store; an unclassified field fails. *Files:* `crates/reify-kernel-gmsh/src/kernel_real.rs`. *Deps:* none.

**INV-GEO-1 — 4876 cluster**
- **ν** — characterize the 4876 OCCT fixture via the §3 validator + `weldedness()` (real input → concrete witness). *Signal:* `validate()`/`weldedness()` on the real 4876 tessellation reports the specific witness (counts); characterization committed. *Files:* `crates/reify-eval/tests/fea_face_selector_bc_e2e.rs`. *Deps:* α.
- **#4876** (adopt) — watertightness preflight in the attributed path → `Err` → graceful degrade + diagnostic (fail-closed immediately). *Signal (b):* `fea_face_selector_bc_e2e` boundary-demand test un-ignored → graceful degrade + visible diagnostic, no SIGSEGV. *Files:* `crates/reify-kernel-gmsh/src/mesh_boundary.rs`, `crates/reify-kernel-gmsh/src/repair.rs`, `crates/reify-eval/tests/fea_face_selector_bc_e2e.rs`. *Deps:* α, ν.
- **ξ** — real fix: attribution-aware repair (vertex-merge correspondence map through `repair_surface_mesh`). *Signal:* §5 gmsh-attributed conformance arm un-ignored → real attributed volume mesh, attribution preserved. *Files:* `crates/reify-kernel-gmsh/src/repair.rs`, `crates/reify-kernel-gmsh/src/mesh_boundary.rs`. *Deps:* ν, ε, #4876.

### Dependency view
```
                 ┌─→ β ─┐
α ─┬─────────────┼─→ γ ─┴─→ δ            (INV-GEO-1 validator+wire+rollout)
   │             │
   ├─→ ε ─→ ζ ─→ η                        (INV-GEO-2 conformance + extract_edges)
   │        │
   └─→ ν ─┬─┼──────────→ #4876 ─→ ξ       (INV-GEO-1 4876 cluster; ξ also ← ε)
          └─┴──────────────────────┘

θ ─→ ι                                     (INV-GEO-4 stub/real)
κ ─→ λ ;  μ2 ;  μ3                         (INV-GEO-3 warm-start)
```

## §13 — Open (tactical) questions

1. **Validator cost in production.** `validate()` is O(V+E) once per Mesh handoff (not per frame). At
   the ratified enforce-everywhere posture this runs on every realization; if a pathological
   large-mesh case shows up in profiling, the escape is `REIFY_MESH_CONTRACT=warn` plus a future
   sampled/`debug_assertions`-only mode — decide only if measured, not pre-emptively.
2. **Weld tolerance value.** The position-quotient uses a tolerance; bit-exact (Manifold's choice)
   vs a small epsilon changes which near-coincident verts merge. Pin the default to Manifold's
   existing bit-exact weld for consistency; expose per-call `tol` for the tessellation path. Decide
   at α.
3. **`witness` payload size.** One offending edge/triangle is enough for the diagnostic; if triage
   later wants the full offending set, widen `MeshWitness` additively. Start minimal.
4. **State-inventory as trait vs per-kernel test.** §6/§12 implement it per-kernel (compiler-enforced
   locally). A shared `WarmStateInventory` trait could unify them later; not worth the cross-crate
   coupling now.
