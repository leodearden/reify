# PRD Brief — P4: Unify region references across selectors & FEA targets

> **Brief for a `/prd` author session** (not a finished PRD). Read `./00-findings.md` FIRST.
> **Wave 2** — do NOT author until **P0 is committed**; this PRD *implements P0's region-reference
> model* on the FEA consumer seam. Touches a load-bearing seam (FEA + selectors) — likely B+H.
>
> **Do NOT touch task 3523 or esc-3523-75/76.** Today is 2026-06-24. Line numbers accurate at time
> of writing — G3-verify against current `main`.

## Why this PRD exists

FEA Load/Support geometry targets are a **fourth, disconnected naming namespace**: opaque strings
(`PointLoad(point:"tip")`, `FixedSupport(target:"root")`, `PressureLoad(face:"x_max")`) validated
as plain strings by `validate_selector_target` (`crates/reify-stdlib/src/helpers.rs:214`), which
**rejects `Value::Selector` and `Value::Frame`** (findings §3/§5). So a `let`-bound selector or an
`@face` datum cannot be passed as a load target. This PRD bridges FEA targeting onto P0's unified
region-reference model so naming composes across the FEA boundary.

## Scope / deliverables

1. **Bridge `validate_selector_target`** to accept P0's canonical region-reference value (and
   `Value::Frame` where a pose is the right input), eliminating the FEA-only string namespace.
2. **Resolution path** from a region reference to the FEA node/element set the solver consumes
   (selector → handle-set → DOF mapping), reusing the existing selector resolution rather than a
   parallel string match.
3. **Migration** of the existing FEA stdlib `structure def`s (the `String` placeholder fields the
   `topology-selector-value-type` PRD already named as `FaceSelector`/`BodySelector` targets) to the
   typed reference, preserving existing call-site syntax where possible.
4. A **boundary test** facing both the selector producer and the FEA consumer (the
   `tests/prd-gate/fixtures/r3b_displacement_at_selector_grammar.ri` negative fixture is the current
   "selector rejected where a string is expected" guard — flip it once the bridge lands).

## Design questions to resolve

- Exactly which P0 reference type FEA accepts, and whether `Value::Frame` (a pose) and a region
  reference (a set) are *both* valid target inputs with distinct meaning (point-load-at-frame vs
  pressure-on-face-set).
- How this composes with the v0.6 FEA selector migration already in flight
  (`docs/prds/v0_6/fea-load-support-selector-migration.md`) — coordinate ownership (G4); do NOT
  re-file its work here.
- Backward compatibility for existing string-target `.ri` models (deprecation path vs dual-accept).

## Key code pointers (verify against current main)

- FEA target validation: `crates/reify-stdlib/src/helpers.rs:214` (`validate_selector_target`,
  accepts only `Value::String | Value::Map`).
- FEA stdlib structure defs / placeholders: `docs/prds/topology-selector-value-type.md` §1 (names
  the `FaceSelector`/`BodySelector` intent); the `fea_*` examples under `examples/`.
- In-flight FEA selector migration: `docs/prds/v0_6/fea-load-support-selector-migration.md`.
- Negative fixture: `tests/prd-gate/fixtures/r3b_displacement_at_selector_grammar.ri`.

## Out of scope

- The region-reference model itself → **P0** (this PRD consumes it). `Feature`/provenance → P1/P3.
- The substrate cleanup → **P2**.

## Dependencies

- **Upstream:** **P0** (the model). Coordinate with the v0.6 FEA migration PRD (G4 seam ownership).
  Wire real `add_dependency` edges to P0's tasks.
- **Downstream:** FEA workflows that target named regions.

## SOP reminders

- Commit the PRD before tasks. This is a cross-PRD seam (FEA migration) — resolve ownership in the
  PRD's Cross-PRD relationship table (G4). Cite `./00-findings.md`. Every named deliverable = a leaf
  task with a file-exists + content signal.
