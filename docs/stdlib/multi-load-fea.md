# Multi-Load-Case FEA

**Applies to:** Reify v0.4.x
**Status:** Shipped — all types and functions described here are live as of v0.4.x.
**Audience:** Users running structural analysis against multiple load scenarios and needing stress envelopes or design-code load combinations.
**Not a PRD:** For design rationale and decomposition see [`docs/prds/v0_3/multi-load-case-fea.md`](../prds/v0_3/multi-load-case-fea.md) (multi-load API design) and [`docs/prds/v0_4/fea-result-model.md`](../prds/v0_4/fea-result-model.md) (result-model implementation; the `multi_load_bracket` example is task η of that PRD).

---

Real engineering FEA rarely has a single load case. A mounting bracket sees an operating load, a shipping-drop acceleration, and a safety-factor overload. A pressure vessel is tested at working pressure, proof pressure, and burst. This page covers the `std.fea.multi_case` stdlib surface that makes multi-load analysis a one-liner rather than manual orchestration.

**See also:** [`docs/fea-cache.md`](../fea-cache.md) for cache-key details and `reify cache` subcommand reference; [`examples/multi_load_bracket.ri`](../../examples/multi_load_bracket.ri) for the validated end-to-end example; [`docs/getting-started.md`](../getting-started.md) for a broader introduction to Reify.

---

## 1. Basic pattern

The three-step workflow is: build `LoadCase` bundles → call `solve_load_cases` → query envelope.

```
// ── Material and geometry ──────────────────────────────────────────────────
let material  = Steel_AISI_1045()

// Prismatic plate: 80 mm wide × 100 mm tall × 6 mm thick
let width     = 80mm
let height    = 100mm
let thickness = 6mm

// ── Load-case bundles ──────────────────────────────────────────────────────
// Shared mounting face; only the applied load varies between cases.
let mount = FixedSupport("mount_face")

let operating = LoadCase(
    name:     "operating",
    loads:    [PointLoad(point: "load_face", force: 5000.0,  direction: [0.0, -1.0, 0.0])],
    supports: [mount],
)

let overload = LoadCase(
    name:     "overload",
    loads:    [PointLoad(point: "load_face", force: 10000.0, direction: [0.0, -1.0, 0.0])],
    supports: [mount],
)

let transport = LoadCase(
    name:     "transport",
    loads:    [Gravity(magnitude: 5 * STANDARD_GRAVITY(), direction: [0.0, -1.0, 0.0])],
    supports: [mount],
)

// ── Solve all cases ────────────────────────────────────────────────────────
let results = solve_load_cases(
    material, width, height, thickness,
    [operating, overload, transport],
    ElasticOptions(shell_force: ShellForce.Off),
)

// ── Envelope and design predicate ─────────────────────────────────────────
let envelope     = envelope_von_mises(results)   // Field<Point3, Pressure>
let peak_stress  = max(envelope)                 // Scalar<Pressure>
let yield_limit  = 310MPa                        // Steel_AISI_1045 yield stress
let within_yield = peak_stress < yield_limit     // Bool
```

*(Verbatim from [`examples/multi_load_bracket.ri`](../../examples/multi_load_bracket.ri),
validated by
[`crates/reify-compiler/tests/multi_load_bracket_example_tests.rs`](../../crates/reify-compiler/tests/multi_load_bracket_example_tests.rs)
and
[`crates/reify-eval/tests/multi_load_bracket_e2e.rs`](../../crates/reify-eval/tests/multi_load_bracket_e2e.rs).)*

### `solve_load_cases` signature

```
pub fn solve_load_cases(
    material : ConstitutiveLaw,
    length   : Length,
    width    : Length,
    height   : Length,
    cases    : List<LoadCase>,
    options  : ElasticOptions = ElasticOptions()
) -> MultiCaseResult
```

The first four parameters (`material`, `length`, `width`, `height`) mirror the opening parameters of `solve_elastic_static` — the same prismatic-geometry bindings thread through unchanged. `cases` is a `List<LoadCase>`; `solve_load_cases` enforces unique names at solve time. `options` is the fallback `ElasticOptions` for any case whose own `options` field is `none`.

**Parameter-name mapping for the example:** the §1 snippet binds `width=80mm`, `height=100mm`, `thickness=6mm` (the bracket's own geometry fields) and passes them positionally to the signature's `length`, `width`, `height` parameters — so `width(80mm)` fills `length`, `height(100mm)` fills `width`, and `thickness(6mm)` fills `height`. The example comment in [`examples/multi_load_bracket.ri`](../../examples/multi_load_bracket.ri) spells this out ("length=width, width=height, height=thickness").

**Current geometry limitation:** the geometry is prismatic (rectangular-box dimensions). The full arbitrary-geometry variant — `solve_load_cases(body, material, cases, options)` where `body` is a CSG solid and loads attach via `body.face()` topology selectors — is deferred to task 2930 (FEA-in-the-loop optimisation, arbitrary-geometry producers not yet wired). For now, callers pass the four dimensional scalars.

### Why `ShellForce.Off`?

The default `ShellForce.Auto` classifies the bracket as a shell (thickness/in-plane ratio 6/80 = 0.075 < the shell threshold of 0.2). The shell kernel handles only transverse (Z-axis) loads; the example's Y-direction loads (`direction: [0.0, -1.0, 0.0]`) would be silently discarded. `ShellForce.Off` forces the tet/solid path, which handles all three force-vector components and produces the `Regular3D` Sampled stress fields that `envelope_von_mises` requires.

A thin-body advisory (aspect ratio ≫ 10) fires at `Warning` severity — not `Error` — and is expected for plate-like geometry. It does not prevent the solve.

### `LoadCase` fields

| Field | Type | Default | Notes |
|---|---|---|---|
| `name` | `String` | — | Unique key in the output `MultiCaseResult.cases` Map; uniqueness enforced by `solve_load_cases`. |
| `loads` | `List<Load>` | — | Each element must conform to `trait Load` (`PointLoad`, `PressureLoad`, `Gravity`, …). Enforced at compile time. |
| `supports` | `List<Support>` | — | Each element must conform to `trait Support` (`FixedSupport`, …). Enforced at compile time. |
| `options` | `Option<ElasticOptions>` | `none` | Per-case solver-knob override. `none` inherits the shared `options` from `solve_load_cases`. |

### `MultiCaseResult` and its accessors

`MultiCaseResult` carries `cases : Map<String, ElasticResult>` keyed by `LoadCase.name`. Keys are returned in stable lexicographic (BTreeMap) order.

| Accessor | Signature | Notes |
|---|---|---|
| `case_names` | `(MultiCaseResult) -> List<String>` | Keys in lexicographic order. Deterministic. |
| `result_for` | `(MultiCaseResult, String) -> ElasticResult` | Returns `Undef` on a missing key (silent-Undef; actionable diagnostics deferred to PRD task #10). |

---

## 2. Envelope construction

The envelope across N cases answers: *at each point in the body, what is the worst-case value of some scalar under any load?*

### Convenience helpers (reach for these first)

| Helper | Return type | What it envelopes |
|---|---|---|
| `envelope_von_mises(results)` | `Field<Point3, Pressure>` | Per-point max von Mises stress across all cases |
| `envelope_max_principal(results)` | `Field<Point3, Pressure>` | Per-point max (most-tensile) principal stress across all cases |
| `envelope_displacement_magnitude(results)` | `Field<Point3, Length>` | Per-point max displacement vector magnitude across all cases |

All three share the same silent-Undef contract: shape failures (mismatched Sampled-grid metadata across cases, wrong arity, non-`MultiCaseResult` argument) collapse to `Undef` rather than raising a diagnostic. Actionable diagnostics are deferred to PRD task #10.

**Round-trip properties:**

| Helper | Identity |
|---|---|
| `envelope_von_mises` | `result[P] == max over cases of von_mises(cases[name].stress[P])` |
| `envelope_max_principal` | `result[P] == max over cases of max_eigenvalue(cases[name].stress[P])` |
| `envelope_displacement_magnitude` | `result[P] == max over cases of \|cases[name].displacement[P]\|` |

### Compositional primitives: `envelope_max` and `envelope_min`

```
envelope_max(fields : Map<String, Field<Point3, T>>) -> Field<Point3, T>  // T : Ordered
envelope_min(fields : Map<String, Field<Point3, T>>) -> Field<Point3, T>  // T : Ordered
```

`envelope_max` and `envelope_min` reduce a named map of per-case scalar fields to a point-wise maximum or minimum across the case axis. They are the underlying building blocks for all three convenience helpers: `envelope_von_mises` is `envelope_max` composed with the built-in per-case von Mises projection; `envelope_max_principal` is `envelope_max` over the per-case max-eigenvalue projection; `envelope_displacement_magnitude` is `envelope_max` over the per-case displacement vector magnitude. The round-trip identities in the table above hold because each helper is a thin wrapper over the shared Rust `envelope_reduce` kernel.

**When to reach for which:**

- Use the three **convenience helpers** (`envelope_von_mises`, `envelope_max_principal`, `envelope_displacement_magnitude`) for the three common projections — they compose the per-case projection internally in Rust and are pre-validated against the `MultiCaseResult` shape.
- **Per-case displacement magnitude:** use `envelope_displacement_magnitude`. There is no standalone user-callable function that extracts a per-case displacement magnitude field from an `ElasticResult` for direct composition with `envelope_max`.
- **Custom scalar projections (v0.3 limitation):** v0.3 ships no standalone user-callable per-case projection that you could supply to `envelope_max` directly. The per-case von Mises, max-principal, and displacement-magnitude projections exist only inside the convenience helper implementations in Rust (`crates/reify-stdlib/src/fea.rs`). `envelope_max` and `envelope_min` remain useful when you have a `Map<String, Field<Point3, T>>` built from another source, but extracting per-case field data from an `ElasticResult` in the current Reify grammar is not yet supported. Future releases will generalise field access on `ElasticResult` (see task #2930).

**Shared-grid-metadata contract:** all per-case fields passed to envelope primitives must share identical Sampled-grid metadata (grid kind, axis lengths, bounds, spacing, `domain_type`, `codomain_type`). This is the same contract as `linear_combine`. Mismatched grids collapse to `Undef`.

### `worst_case` — find the governing case name

```
worst_case(results : MultiCaseResult, scalar_fn : (ElasticResult) -> Field<Point3, T>) -> String
```

Returns the name of the case whose `scalar_fn`-derived field has the largest global maximum. Tie-break: lexicographic-min on the case name (first-seen in BTreeMap alphabetical iteration order, same discipline as `envelope_reduce`).

**Identity-lambda caveat (task #3007):** the current Reify lambda parameter-type syntax accepts only bare named types resolvable by `resolve_type_name`. Untyped lambda params default to `Type::Real`, so `|e| e["displacement"]` is rejected by the type checker ("cannot index into non-collection type 'Real'"). Until richer lambda parameter-type syntax lands, callers must pre-bind the desired scalar field and pass the identity lambda `|f| f`:

```
// Current idiom: the identity lambda; worst_case applies it per case
// (intercepted in reify-expr::eval_expr, which has EvalContext).
let critical_case = worst_case(results, |r| r)
```

This idiom is exercised in [`crates/reify-eval/tests/multi_load_case_stdlib_smoke.rs`](../../crates/reify-eval/tests/multi_load_case_stdlib_smoke.rs).

---

## 3. Superposition (`linear_combine`)

For **linear-elastic** FEA the superposition identity holds:

> result(α·A + β·B) = α·result(A) + β·result(B)

This means any weighted combination of pre-solved cases is pure field arithmetic — no re-solve required. `linear_combine` implements this:

```
linear_combine(
    base_results : MultiCaseResult,
    weights      : Map<String, Number>,
) -> ElasticResult
```

The output `ElasticResult` has:

| Field | Value |
|---|---|
| `displacement` | Weighted-sum Sampled field (`name = "linear_combine"`) |
| `stress` | Weighted-sum Sampled field (`name = "linear_combine"`) |
| `max_von_mises` | `max` over the combined stress field's finite values |
| `converged` | `true` (synthesised, not solved) |
| `frame` | `Undef` (tet convention — no per-element local frame for solid elements) |
| `iterations` | `Undef` (synthesised, not solved) |

### LRFD combination sweep

A typical structural design code (LRFD / Eurocode) mandates checking several factored load combinations. With pre-solved bases these are cheap:

```
// Pre-solved bases: "operating" (live load), "transport" (dead/inertial), "overload" (2× live)

// ASCE 7-style factored combinations (illustrative — apply project-specific factors):
let combo_1 = linear_combine(results, map{"transport" => 1.4})
    // 1.4D — dead load only

let combo_2 = linear_combine(results, map{"transport" => 1.2, "operating" => 1.6})
    // 1.2D + 1.6L — dominant live load

let combo_3 = linear_combine(results, map{"transport" => 1.2, "overload"  => 1.0})
    // 1.2D + 1.0E — notional seismic / accident event

let combo_4 = linear_combine(results, map{"transport" => 0.9, "overload"  => 1.0})
    // 0.9D + 1.0E — uplift check (dead resists overturning)

// From the validated bracket example (ACI 318 legacy factors):
let lrfd = linear_combine(results, map{"transport" => 1.4, "operating" => 1.7})
```

Each call returns an `ElasticResult` that can be queried directly:

```
// Peak stress in the dominant-live-load combination:
let combo_2_peak = max(envelope_von_mises(MultiCaseResult(cases: map{"combo_2" => combo_2})))
let combo_2_ok   = combo_2_peak < yield_limit
```

Each `linear_combine` call is cheap field arithmetic over pre-solved `SampledField.data` buffers — no solver invocation, no mesh work. For a 10–20-row design-code combination sweep, all combinations complete in milliseconds. See §5 for the cost model.

### The linear-elastic-only constraint

**`linear_combine` is valid for linear-elastic results only.** The superposition identity holds because linear-elastic analysis imposes no state that carries between load applications: no plasticity, no contact, no geometric stiffness from large deformations. Applying superposition to a non-linear analysis produces incorrect results without warning.

The v0.4+ non-linear solver result types (plasticity, contact, large-deformation) will **not** provide a `linear_combine` overload — the absence of the overload at the type level is the machine-checked enforcement. If you migrate a design from linear-elastic to a non-linear solver and attempt `linear_combine`, the call will fail to resolve at compile time.

### Mesh-compatibility pre-check

All referenced base cases must share identical Sampled-field grid metadata (grid kind, axis lengths, bounds, spacing). If grids differ — for example because two cases used different `mesh_size` overrides — `linear_combine` returns `Undef`. Actionable diagnostics for this mismatch are deferred to PRD task #10.

See §4 for the per-case `mesh_size` / `element_order` option interaction and the full compatibility matrix.

---

## 4. Per-case options compatibility matrix

`LoadCase.options : Option<ElasticOptions> = none` lets individual cases override solver knobs. `none` (the default) inherits the shared `options` argument passed to `solve_load_cases`.

| Option override | Per-case OK? | Effect on envelope / superposition |
|---|---|---|
| `cg_tolerance` | ✓ yes | Per-case independent; envelope and superposition unaffected |
| `max_iter` | ✓ yes | Per-case independent; envelope and superposition unaffected |
| `threads` | ✓ yes | Per-case independent; envelope and superposition unaffected |
| `#deterministic` | ✓ yes | Per-case independent; envelope and superposition unaffected |
| `mesh_size` | ⚠ allowed but disables superposition | Different DOF layout per case; `linear_combine` returns `Undef` (diagnostic deferred to PRD task #10) if base meshes differ |
| `element_order` | ⚠ allowed but disables superposition | Different DOF layout per case; `linear_combine` returns `Undef` if base meshes differ |

*(Adapted from [`docs/prds/v0_3/multi-load-case-fea.md`](../prds/v0_3/multi-load-case-fea.md). Note: the PRD's wording for `mesh_size`/`element_order` conflicts — "rejects with diagnostic if base meshes differ" — is aspirational; the shipped v0.3 contract is silent-`Undef` with diagnostics deferred to PRD task #10, as shown in the table above.)*

**Practical guidance:** keep `mesh_size` and `element_order` identical across all cases (or leave them as `none` to inherit the shared options) unless you have a specific reason to vary element fidelity per case. Mixing mesh sizes is valid for running a coarse sanity-check case alongside fine-mesh production cases, but rules out `linear_combine` across those cases.

**Example — per-case `cg_tolerance` override (safe, envelope and superposition unaffected):**

```
let tight_case = LoadCase(
    name:     "fine_check",
    loads:    [PointLoad(point: "load_face", force: 5000.0, direction: [0.0, -1.0, 0.0])],
    supports: [mount],
    options:  some(ElasticOptions(cg_tolerance: 1e-10, shell_force: ShellForce.Off)),
)
// fine_check contributes to envelope_von_mises and linear_combine as normal.
```

**Example — per-case `mesh_size` override (disables superposition for that pair):**

```
let coarse_case = LoadCase(
    name:     "coarse_sanity",
    loads:    [PointLoad(point: "load_face", force: 5000.0, direction: [0.0, -1.0, 0.0])],
    supports: [mount],
    options:  some(ElasticOptions(mesh_size: 0.02, shell_force: ShellForce.Off)),
)
// linear_combine(results, ...) returns Undef because coarse_sanity's grid
// metadata differs from the other cases (different DOF layout).
// envelope_von_mises also returns Undef for the same reason.
// Inspect coarse_sanity independently via result_for(results, "coarse_sanity").
```

---

## 5. Performance notes

### Volume-mesh cache reuse

When `material`, `length`, `width`, `height` (the first four `solve_load_cases` parameters) together with the shared `options.mesh_size` and `options.element_order` are identical across all cases, the volume-mesh ComputeNode cache hits **once** and is reused for every case's assembly step. The v0.3 cache key excludes `loads` and `supports` from the mesh hash — boundary conditions do not affect the meshing step.

This makes the canonical "shared BCs, per-case load variation" shape (like the bracket example) as cache-efficient as a single-case solve: meshing is paid once regardless of how many cases are stacked on top.

For cache-key details, storage layout, environment variables, and the `reify cache` subcommand reference (stats / clear / gc / export / import), see [`docs/fea-cache.md`](../fea-cache.md).

### Warm-start chain

The **first case** pays the full cost: mesh generation → matrix assembly → factorisation → back-substitution.

**Subsequent cases** within the same `solve_load_cases` call incur:

| Step | Cost when supports are unchanged | Cost when supports vary |
|---|---|---|
| Mesh | Free (cache hit) | Free (cache hit) |
| Matrix assembly | Free (stiffness matrix identical) | Paid (BCs change the assembled system) |
| Factorisation | Free (reused from first case) | Paid per distinct support configuration |
| Back-substitution | Paid (right-hand side changes with loads) | Paid |

In the bracket example — three cases sharing one `FixedSupport("mount_face")`, with only the load vector differing — the marginal cost per additional case is one back-substitution. The mesh and factorisation are paid once for the three cases combined.

If `supports` vary between cases (e.g. one pinned case and one fixed-base case), the assembled system matrix differs and factorisation is repeated per distinct support configuration. Load variation within a fixed support configuration is always cheap.

### `linear_combine` is always cheap

`linear_combine` performs pure field arithmetic over pre-solved `SampledField.data` buffers — no solver call, no mesh work. Cost scales as O(N\_cases × N\_gridpoints), not as O(N\_solver\_dof). A 10–20-row LRFD combination sweep over pre-solved cases completes in milliseconds.

---

## References

- [`docs/prds/v0_3/multi-load-case-fea.md`](../prds/v0_3/multi-load-case-fea.md) — PRD with design rationale, resolved decisions, and decomposition.
- [`docs/fea-cache.md`](../fea-cache.md) — FEA cache key details, `reify cache` subcommand reference, performance caveats.
- [`examples/multi_load_bracket.ri`](../../examples/multi_load_bracket.ri) — validated end-to-end example: three load cases, stress envelope, LRFD superposition.
- [`crates/reify-compiler/stdlib/fea_multi_case.ri`](../../crates/reify-compiler/stdlib/fea_multi_case.ri) — authoritative type and function declarations with full doc-comments (`structure def LoadCase`, `structure def MultiCaseResult` and its accessors, `pub fn solve_load_cases`, and the `linear_combine` / `envelope_*` / `worst_case` function doc-blocks).
- [`docs/getting-started.md`](../getting-started.md) — broader introduction to Reify.
