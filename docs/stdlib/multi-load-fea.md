# Multi-Load-Case FEA

**Applies to:** Reify v0.3.x
**Status:** Shipped — all types and functions described here are live as of v0.3.x.
**Audience:** Users running structural analysis against multiple load scenarios and needing stress envelopes or design-code load combinations.
**Not a PRD:** For design rationale and decomposition see [`docs/prds/v0_3/multi-load-case-fea.md`](../prds/v0_3/multi-load-case-fea.md).

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

### Compositional primitives (reach for these for any other scalar field)

```
envelope_max(fields : Map<String, Field<Point3, T>>) -> Field<Point3, T>  // T : Ordered
envelope_min(fields : Map<String, Field<Point3, T>>) -> Field<Point3, T>  // T : Ordered
```

`envelope_max` and `envelope_min` reduce a named-field map to a point-wise maximum or minimum across the case axis. Compose them with any per-case scalar projection to envelope a custom quantity:

```
// Example: per-point max of a scalar derived from each case's displacement field
let disp_fields = map{
    "operating" => displacement_magnitude_field(result_for(results, "operating")),
    "overload"  => displacement_magnitude_field(result_for(results, "overload")),
    "transport" => displacement_magnitude_field(result_for(results, "transport")),
}
let worst_disp = envelope_max(disp_fields)
```

**When to reach for which:**

- Use the three **convenience helpers** (`envelope_von_mises`, `envelope_max_principal`, `envelope_displacement_magnitude`) for the common projections — they are shorter and pre-validated against the `MultiCaseResult` shape.
- Use `envelope_max` / `envelope_min` for any **other scalar field** derived from per-case `ElasticResult` data; the helpers are one-liners over these primitives.

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
