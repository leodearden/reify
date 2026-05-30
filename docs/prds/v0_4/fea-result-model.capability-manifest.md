# Capability Manifest — fea-result-model.md

Mechanizes G3 + G6 per leaf (overlay → *Capability Manifest — reify evidence forms*). Each task's user-observable/RED signal is decomposed into asserted capabilities, each bound to evidence ∈ `{grep:file:line-wired | producer:task-upstream | grammar-fixture:parses | floor:bound>X | field-population}`. A binding resolving to `{declared-only | test-only | producer-absent | producer-downstream | fixture-ERROR | bound≤floor}` **blocks** queueing until resolved.

Sentinel for this PRD: `Value::Undef` (and the `{ ElasticResult() }` stub body, `scalar_channels: HashMap::new()`, `displaced_positions: None`). Evidence current as of 2026-05-30; G3 fixtures `/tmp/prd-gate-fixtures/fea-result-model-{1,2}.ri` parse with 0 ERROR nodes.

**Status legend:** ✅ PASS · ⏳ FAIL-today-resolved-by-this-batch (in-batch producer is upstream; DAG-correct) · ⛔ BLOCK (must resolve before queue).

---

## α — Populate `ElasticResult.{stress,displacement}` as Sampled fields  *(intermediate)*

| Capability | Evidence | Status |
|---|---|---|
| `recover_nodal_stress_p1` | `grep:reify-solver-elastic/src/result.rs:356` · `producer:2920(done)` | ✅ |
| `interpolate_p1_at_point` / `locate_element_p1` / `barycentric_p1` | `grep:reify-solver-elastic/src/interpolation.rs:144/189/51` · `producer:2920(done)` | ✅ |
| `Value::Field{source:Sampled}` + `SampledField` Regular3D | `grep:reify-ir/src/value.rs:89,316,486` | ✅ |
| `Matrix3x3<Pressure>` / `Vector3<Length>` codomain types | `grep:reify-stdlib/src/fea.rs:451-477` (TensorShape extract_quantity) | ✅ |
| **field-population**: `ElasticResult.stress`/`displacement` write a **non-`Undef` Sampled** value on the production path | today `Value::Undef` (`elastic_static.rs:196-197`, `buckling.rs:260-261`) — **α IS the producer that fixes it** | ⏳ resolved-by-α |

α is the linchpin: every field-sampling consumer below binds its field-population check to `producer:α-upstream`. No consumer leaf may assert sampling `stress`/`displacement` unless it depends on α.

## β — Reduce `VonMises`-derived fields in `max`/`min`/`argmax`/`argmin`  *(leaf: `max(von_mises(stress))→Scalar<Pressure>`, not `Undef`)*

| Capability | Evidence | Status |
|---|---|---|
| `von_mises(Field)→Field` (VonMises-derived) | `grep:reify-expr/src/analysis.rs:157` wired `lib.rs:356` | ✅ |
| single-field `max(Field)→Scalar` over **Sampled** | `grep:reify-expr/src/field_reductions.rs:82-95` wired `lib.rs:404` | ✅ |
| `max` reduces **`VonMises`-derived** source | today returns `Undef` (`field_reductions.rs:101-115`, deferred) — **β IS the producer** | ⏳ resolved-by-β |
| projection kernel reuse `compute_von_mises_3x3` | `grep:reify-stdlib/src/analysis.rs:38` (`pub(crate)`) | ✅ |
| grammar `max(von_mises(stress))` + `constraint <` | `grammar-fixture:fea-result-model-1.ri` (0 ERROR) | ✅ |
| numeric: `≈ analytical σ_max` | `floor:` reuse existing **±50%-of-6 MPa P1-tet bending-lock budget** (`solve_elastic_static_e2e.rs`) — assert capability (non-`Undef`) + that budget, **not** a tighter guess | ✅ |

## γ — GUI engine FEA dispatch wiring (cap v / esc-2962-66)  *(intermediate)*

| Capability | Evidence | Status |
|---|---|---|
| `register_compute_fns` | `grep:reify-eval/src/compute_targets/mod.rs:29` (exists) — but **zero GUI call sites** (test-only) → GUI solve body-inlines to stub | ⏳ wired-on-main FAIL → **γ registers it in `Engine::new`/`from_engine`** |
| `pending_solve_cancel` **producer** | consumer exists `grep:gui/src-tauri/src/commands.rs:321-333`; **no producer** (`:59` always `None`, `main.rs:655`) | ⏳ producer-absent → **γ sets `Some(handle)` on solve start** |
| FEA fixture geometry realization | `examples/fea_cantilever_smoke.ri` has **no `body =`** | ⏳ **γ adds `body = box(length,width,height)`** |

## δ — GUI `ElasticResult`→`scalar_channels`/`displaced_positions` (cap iii; M-006/M-010)  *(intermediate)*

| Capability | Evidence | Status |
|---|---|---|
| `MeshData.scalar_channels` / `displaced_positions` IPC schema | `grep:gui/src-tauri/src/types.rs:250,261` (+ TS mirror) | ✅ |
| **field-population** of those slots | emit-site hardcoded empty `grep:gui/src-tauri/src/engine.rs:1921-1922` | ⏳ producer-absent → **δ samples the Sampled fields at surface vertices** (`producer:α-upstream` for the fields) |
| per-vertex von Mises | `compute_von_mises_3x3` `grep:reify-stdlib/src/analysis.rs:38` | ✅ |

## ε — 2962 (re-homed): readout + per-vertex contour + Lock Current  *(LEAF — C-as-integration-gate, names §5 boundary table)*

| Capability | Evidence | Status |
|---|---|---|
| ElasticResult reaches GUI with real `max_von_mises` | `producer:γ-upstream` | ⏳ via γ |
| contour render path (scalar_channels populated) | `producer:δ-upstream` | ⏳ via δ |
| FEA-mode toggle / colormap / range UI | `grep:gui/src/stores/feaModeStore.ts`, `viewport/colormap.ts` (M-008/M-009 WIRED, 2961 done) | ✅ |
| Lock Current handler (pure frontend) | `grep:gui/src/viewport/Viewport.tsx` (empty TODO, M-009 note) — **ε delivers, no kernel dep** | ✅ self-delivered |

## ζ — 3015 superposition validation  *(leaf)*

| Capability | Evidence | Status |
|---|---|---|
| `linear_combine(MultiCaseResult, weights)` | `grep:reify-stdlib/src/fea.rs:109` wired | ✅ |
| **field-population**: combines real Sampled stress/displacement (not `Undef`) | today vacuous over `Undef` | ⏳ `producer:α-upstream` |
| numeric: error `< Σ\|w\|·cg_tol·C` | `floor:` **derived** from CG tolerance + weight-sum (documented in 3015 test comment) — not a guessed absolute; rate/derived bound | ✅ |

## η — 3018 `multi_load_bracket` example  *(leaf)*

| Capability | Evidence | Status |
|---|---|---|
| `solve_load_cases` real per-case results | `producer:R1-upstream` | ⏳ via R1 |
| `envelope_von_mises` / `envelope_max_principal` | `grep:reify-stdlib/src/fea.rs:47-48` wired | ✅ |
| **field-population**: per-case Sampled fields | `producer:α-upstream` | ⏳ via α |
| grammar `minimize … where …`, `face(body,…)` | `grammar-fixture:fea-result-model-2.ri` (0 ERROR) | ✅ |
| **bracket-geometry** solve + face-selector BC semantics | `producer:P1/P2/3429` (structural-analysis-fea) — **arbitrary geometry** | ⛔ → ship the **prismatic multi-case** variant now; bracket-geometry variant **gated** (split or hold) |

## θ — 3026 GUI case-picker dropdown  *(leaf)*

| Capability | Evidence | Status |
|---|---|---|
| per-case re-source of contour | `producer:δ,ε,R1-upstream` | ⏳ via δ/ε/R1 |
| MultiCaseResult detection at GUI boundary | new GUI plumbing (θ delivers); rides FEA-mode toggle (2961 done) | ✅ self-delivered |
| per-case visual baselines | viewport capture OK; **full-window** scenes need `screenshot_window` (M-001 FICTION) — drop full-window asserts | ⏳ scope to viewport capture |

## R1 — `solve_load_cases` real ComputeNode multi-case lowering + cache-reuse  *(intermediate + leaf: B9)*

| Capability | Evidence | Status |
|---|---|---|
| `@optimized` solve_load_cases + `compute_targets/multi_case.rs` trampoline | NEW — R1 delivers (per 3005 architect dry_run Option A); `compute_targets/mod.rs` registry `grep:reify-eval/src/compute_targets/mod.rs:29` | ⏳ R1 delivers |
| real per-case ElasticResult | `producer:α-upstream` | ⏳ via α |
| `realization_entries` cache instrumentation | referenced by 3005 architect dry_run (`engine.cache_stats().realization_entries`) | ✅ |
| `LoadCase`/`MultiCaseResult` types | `producer:3004` | ✅ |

## R2 — per-Support/per-Load source-span provenance (re-home from 2929)  *(leaf: B10)*

| Capability | Evidence | Status |
|---|---|---|
| source-span on `Value::StructureInstance` (PointLoad/FixedSupport) | absent today (2929 relaxed note: span = `None` at solve time) — **R2 adds it to the value model** | ⏳ R2 delivers |
| ComputeFn-signature span threading into `solve_elastic_static_trampoline` | absent today — R2 threads it | ⏳ R2 delivers |
| diagnostic emit path (consumer) | `producer:2929` (messages+DiagnosticCode, pending) — R2 supplies the span 2929's solve-time diagnostic lacked | ✅ consumer ready |

## R3 — typed structured FEA diagnostics  *(intermediate → ι)*

| Capability | Evidence | Status |
|---|---|---|
| `Vec<DofDirection>` / `Vec<ElementId>` / `UnresolvedSelector` typed structs | NEW — **Rust kernel structs** (`reify-solver-elastic/src/diagnostics.rs`), **not Reify-language enums** → no C-style-payload-enum grammar gap (cf. esc-2998 is a *Reify-value* issue, irrelevant here) | ⏳ R3 delivers |
| diagnostic message+code base | `producer:2929` (pending) | ✅ |

## ι — 2966 diagnostic overlay  *(leaf: B11)*

| Capability | Evidence | Status |
|---|---|---|
| typed diagnostics to drive arrows/outlines | `producer:R3-upstream` | ⏳ via R3 |
| `ArrowHelper`/overlay Three.js group | FICTION today (`grep:gui/src` zero hits, M-014) — ι delivers (pure frontend + R3 data) | ⏳ ι delivers |

## κ — 2968 FEA visual-regression baselines (re-scoped to cantilever)  *(leaf)*

| Capability | Evidence | Status |
|---|---|---|
| visual-regression harness + SSIM diff | `grep:gui/test/visual/{run,diff}.ts` (M-005 PARTIAL) | ✅ (harness exists) |
| cantilever contour + deformed scenes | `producer:ε-upstream` | ⏳ via ε |
| pressurised-cylinder / bracket-auto-resolve scenes | gated on arbitrary geometry (P1/P2) + auto-resolve panel producer (M-015 absent) | ⛔ **split out — do not silently fold in** (`log` the dropped scenes; no silent cap) |
| full-window probe/overlay capture | `screenshot_window` FICTION (M-001/2954) | ⛔ gated on harness fix (out of scope) |

## 2930 — bracket auto-thickness minimize-mass (kept a bracket, gated)  *(leaf, cross-PRD gated)*

| Capability | Evidence | Status |
|---|---|---|
| grammar `minimize mass(body) where max(von_mises(fea.stress)) < yield`, `face(body,…)` | `grammar-fixture:fea-result-model-2.ri` (0 ERROR); `minimize_declaration` `grep:tree-sitter-reify/grammar.js:548` | ✅ |
| `max(von_mises(stress))` predicate | `producer:β-upstream` + `producer:α-upstream` | ⏳ via α/β |
| **arbitrary-geometry solve on realized bracket + face-selector BC** | `producer:P1/P2/3429` (structural-analysis-fea) — **not yet filed** | ⛔ **BLOCK until P1/P2 filed upstream + dep wired** (§11 Q4); P1/P2 must be UPSTREAM of 2930 (DAG-direction) |
| `param thickness : Length = auto` | `grep` valid at param-default (overlay note); `grammar-fixture:fea-result-model-2.ri` | ✅ |

---

## Blocking summary (must resolve before/at queue)

1. **2930** — file P1 (trampoline-consumes-realized-mesh) + P2 (face-selector BCs) under `structural-analysis-fea` ownership and wire `2930 → {P1,P2,3429}` so the producer is upstream. Until then 2930 stays `deferred`, not `pending`.
2. **η (3018)** — ship the prismatic multi-case variant in this batch; split/hold the bracket-geometry variant behind the same P1/P2 gate.
3. **κ (2968)** — scope to cantilever contour/deformed; `log` the cylinder/bracket/full-window scenes as explicitly deferred (no silent truncation).

All other ⏳ bindings are resolved **within this batch** by in-batch producers (α/β/γ/δ/R1/R3) that are correctly upstream of their consumers — DAG-direction holds, no inversion.
