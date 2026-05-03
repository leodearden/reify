# PRD: Multi-Load-Case Workflow for FEA

Status: stub — deferred, candidate v0.3.x. Likely a stdlib pattern rather than a language feature. Filed 2026-05-02 from FEA PRD spillover.

## Goal

Make multi-load-case structural analysis ergonomic. Real designs are evaluated against multiple load conditions — operating, transport, accident — and the design constraint is typically "max von Mises across all cases < yield_stress." Currently this requires manual orchestration; should be a one-liner.

## Background

Real engineering FEA workflows almost never use a single load case. Examples:
- A bracket: operating load (5 kN normal), overload (10 kN, 2× safety factor), transport drop (5g acceleration).
- A pressure vessel: working pressure, proof-pressure test, burst test.
- A wing spar: cruise, manoeuvre limit load, gust loads at multiple frequencies.

Designs are typically validated against the *envelope* across all cases — max stress at any point under any load. The auto-resolve loop's design driver is "minimize mass subject to envelope < yield_stress."

The v0.3 FEA PRD (`structural-analysis-fea.md`) ships single-case `solve_elastic_static`. Composing multi-case workflows is technically possible — `forall load in load_cases: solve_elastic_static(...)` then take per-point max — but verbose and easy to get subtly wrong (forgetting a case, mismatching envelope semantics, etc.).

Designers think in load cases. Reify's stdlib should too.

## Why deferred (and separate from FEA PRD)

- v0.3 FEA PRD focused on the fundamental kernel; multi-load is an ergonomic layer on top.
- Design surface decisions (helper function vs. dedicated language form vs. forall pattern) benefit from seeing how the v0.3 single-case API actually feels in use.
- Small enough to land as a stdlib addition once chosen direction is clear.

## Sketch of approach

Two design directions worth comparing:

**Direction A — stdlib helper function:**
```
solve_load_cases(
    body, material,
    cases : List<{name: String, loads: List<Load>, supports: List<Support>}>,
    options : ElasticOptions = .default,
) -> Map<String, ElasticResult>

envelope_stress(results : Map<String, ElasticResult>) -> Field<Point3, Pressure>  // per-point max von Mises across cases
worst_case(results) -> String  // name of the case with max overall von Mises
```

User pattern:
```
results = solve_load_cases(bracket, Steel_AISI_1045, [
    {name: "operating", loads: [...], supports: [...]},
    {name: "overload",  loads: [...], supports: [...]},
    {name: "transport", loads: [...], supports: [...]},
])

minimize mass(bracket) subject to max(envelope_stress(results)) < material.yield_stress
```

**Direction B — dedicated language form:**
```
multi_load bracket : Steel_AISI_1045 {
    case operating  { load = ...; support = ... }
    case overload   { load = ...; support = ... }
    case transport  { load = ...; support = ... }
}
```

Direction B is more aspirational; requires grammar work, less general. Direction A is plain stdlib and composes with existing field reductions.

Lean: **Direction A** for v0.3.x. Revisit Direction B if usage volume justifies dedicated grammar (kinematic-constraints PRD precedent: dedicated grammar earned through usage volume, not speculation).

## Pre-conditions for activating

- v0.3 FEA kernel shipped (`structural-analysis-fea.md` tasks #16, #17, #20).
- Field reductions in stdlib (task #6 of v0.3 FEA PRD).

## Open design questions

- **Direction A vs. B** — see above. Lean A.
- **Per-case provenance display** — when "worst case = transport", how does the user inspect *that case's* full stress field? GUI surface concern (composes with `fea-gui-rendering.md`).
- **Envelope semantics** — per-point max of von Mises is the obvious default. Other reductions: per-point max of any scalar function (max principal stress, signed component stress, displacement magnitude). Each useful in different domains.
- **Load combinations / superposition** — for linear-elastic FEA, results superpose: result(α·load_A + β·load_B) = α·result(load_A) + β·result(load_B). Could be exposed for cheap exploration of load combinations without re-solving. Worth it? Probably yes — single API addition, big speedup for combination sweeps.
- **Per-case options** — can different cases use different `ElasticOptions` (e.g. transport case wants finer mesh)? Lean: yes, per-case options optional.
- **Time-history loading** — sequence of loads representing dynamic events (drop test, vibration profile). Different shape from independent cases; out of scope here, sibling PRD if needed.

## Out of scope for this PRD

- Time-history / transient loading — separate PRD (`structural-analysis-transient.md` if it materialises).
- Load combination factors per LRFD / ASD design codes — domain-specific add-on, post-v0.3.
- Probabilistic / reliability-based load combinations — research-grade, not in scope.
- Auto-detection of load cases from usage context — speculative; users specify their cases explicitly.

## Relationship to other PRDs and tasks

- **Direct extension of `structural-analysis-fea.md`** — uses kernel and field reductions as-is; only adds an ergonomic stdlib layer.
- **Composes with `fea-gui-rendering.md`** — multi-case results need visualization (per-case probe, envelope view, worst-case highlight).
- **Composes with `a-posteriori-error-estimation.md`** — error budget applied per case or shared across cases is a budget-design question.
- **Touches `structural-analysis-shells.md` and `hex-wedge-meshing.md`** — multi-load works the same regardless of element type; ergonomic layer is element-agnostic.
