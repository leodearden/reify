# Capability manifest — `process-dfm-completion`

Mechanizes G3 + G6 per leaf for the PRD `docs/prds/v0_6/process-dfm-completion.md`. Each row binds a
leaf's asserted capability to on-disk evidence. **Any FAIL binding blocks the batch from being
queued.** Evidence forms per the reify `/prd` overlay (anti-orphan/wired, field-population,
grammar-fixture, numeric-floor).

Verification performed at authoring time (2026-06-03) against the working tree with the real
`reify check` binary (`target/debug/reify`); commit hashes are not pinned because the substrate is
pre-existing prelude/dispatch code.

| Leaf | Asserted capability | Evidence form | Evidence (file:line / fixture / cell) | Verdict |
|---|---|---|---|---|
| **α** | `fits_build_volume`/`dfm::diagnose` are **wired** into the builtin dispatch + reify-expr fallthrough, not orphaned | wired-on-main (anti-orphan) | New `dfm::eval_dfm` arm added to the `if let Some(v) = …` chain in `reify-stdlib/src/lib.rs` (sibling of `stackup::eval_stackup` / `tolerancing::eval_tolerancing`); `diagnose` re-exported like `stackup_diagnose`/`flexure_diagnose`; α's signal greps for the arm | **PASS** (seam exists; arm is α's deliverable) |
| **α** | `fits_build_volume` reads real geometry via an **existing** kernel query (no new kernel work) | grammar/substrate (producer-present) | `GeometryQuery::BoundingBox(handle)` exists (`reify-ir/src/geometry.rs:877`); bbox-vs-bbox extent compare is pure arithmetic on two query results | **PASS** |
| **α** | `dfm::diagnose` emits a `DFMSeverity`-mapped diagnostic on the **success** (violation) path | field-population (diagnostic-emission) + precedent | `flexures::flexure_diagnose` "runs on BOTH the success and `Undef` paths" pushing `Severity::Warning`/`Error` into the `EvalContext` sink (`reify-stdlib/src/lib.rs` doc-comment); `dfm::diagnose` copies it. α's test asserts a `Warning` for a `DFMSeverity.Warning` rule | **PASS** |
| **α** | `fits_build_volume` is exact (no numeric floor to mis-set) | numeric-floor (G6) | Bbox extent compare is an exact `≤` on kernel-measured `BoundingBox` reals — no tolerance, no iteration, no closed-form. The hazard-laden measurements (min-wall medial axis, overhang) are **deferred** (stub PRD), keeping α off the G6 numeric branch | **PASS** (no false premise — none asserted) |
| **β** | category-trait capability params compile + a conformer evaluates (field-population) | grammar-fixture + field-population | `/tmp/prd-gate-fixtures/process_dfm_2.ri` → `reify check` exit 0; `MilledBracket : Subtracting` with `tool_access:Solid`/`min_feature_size:Length`/… compiles; member read `proc.min_feature_size` evaluates (non-`Undef`) — proven by the `VIOLATED` constraint firing on the real value | **PASS** (proven by eval) |
| **β** | `tool_access`/`build_volume` typed `Solid` (NOT `Geometry`) | grammar/substrate (negative finding) | `param tool_access : Geometry` → `error: unresolved type: Geometry` (`process_dfm_1.ri`); `Solid` resolves (`type_resolution.rs:563`, `process_dfm_2.ri` exit 0). `Geometry` registration owned by tolerancing #3116 — not claimed here | **PASS** (type chosen; doc-reconcile, not grammar-work) |
| **β** | required-member conformance diagnostic fires when a param is omitted | field-population (negative) | `Process { duration; cost }` already uses the required-member convention on main (no defaults → `required_members`); β's signal asserts an omitting conformer fails | **PASS** |
| **γ** | `Manufacturable` + trait-typed `constraint def` compile + evaluate + report | grammar-fixture | `/tmp/prd-gate-fixtures/process_dfm_3.ri` → `reify check`: `OK Manufacturable#0[0]`, `OK ProcessFeatureOk#0[0]` (trait-typed `param proc : Subtracting` + `proc.min_feature_size` predicate); `VIOLATED` path proven in `process_dfm_2.ri` | **PASS** |
| **γ** | the build-volume DFM rule reads geometry and **flips** pass/fail (field-population, not declared-only) | wired + field-population | calls α's `fits_build_volume` (β-deps-α not needed; **γ deps α**); without α the call is `Undef` (declared-only) → **γ must depend on α**. γ's signal scales the part past the envelope and asserts `OK`→`VIOLATED` | **PASS** (gated on α dep) |
| **γ** | `DFMSeverity`-tagged rule emits the matching `W_/E_DFM_*` diagnostic | wired (anti-inversion) | consumes α's `dfm::diagnose`; γ deps α. Asserts the diagnostic severity matches the rule's `DFMSeverity` | **PASS** (gated on α dep) |
| **δ** | the §8 example runs green in CI (the user-observable leaf) | wired-on-main (integration gate) | `reify check examples/process/std_process_dfm.ri` in CI; shows the `OK`/`VIOLATED` set incl. the build-volume flip + severity diagnostics. Deps γ (α/β transitively) so all producers are landed | **PASS** (gate task) |

## Anti-orphan / anti-inversion summary

- **No orphan producer:** α's `fits_build_volume` is consumed by γ's build-volume rule and the δ
  example; `dfm::diagnose` is consumed by γ's severity-tagged rules — all same-batch, dependency-wired
  (**γ deps α**). The §0 separation guarantees `dfm.rs` is NOT coupled to the kernel-budget, stackup,
  or tolerancing subsystems.
- **No field-population inversion:** γ's geometry-backed rule is non-`Undef` only after α lands; the
  γ-deps-α edge prevents a "declared-only" landing. δ (the CI gate) reads real `OK`/`VIOLATED`
  output, so a silently-`Undef` rule fails the gate (the build-volume flip would not occur).
- **No grammar fiction:** every novel fragment compiled under the real `reify check`
  (`process_dfm_{2,3}.ri` exit 0; the `VIOLATED`/`OK` constraint reports are real evaluation). The one
  negative finding (`Geometry` type unresolvable) is resolved by `Solid`, not deferred to grammar work.
- **No false numeric premise:** this PRD asserts no closed-form numeric floor; `fits_build_volume`
  is an exact bbox compare over an existing kernel query. The hazard-laden measurements (medial-axis
  min-wall, overhang) are explicitly **deferred** to `process-dfm-geometry-metrology.md`, which carries
  a G6 promotion-checklist requiring an honest lower-bound floor before activation.
