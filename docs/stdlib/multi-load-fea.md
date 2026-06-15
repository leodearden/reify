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
