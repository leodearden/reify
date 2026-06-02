# Capability Manifest — `materials-parameter-surface-completion`

Binds each leaf task's asserted capabilities to evidence (mechanizes gates
G3 + G6). All probes run 2026-06-02 against `target/debug/reify` (the real
binary; tree-sitter CLI is stale per project G3 note). Fixtures under
`/tmp/prd-gate-fixtures/`. Any **FAIL** binding blocks queueing — none are FAIL.

| # | Leaf | Capability asserted | Evidence form | Verdict |
|---|---|---|---|---|
| 1 | α/β/γ/δ | `param … = undef` makes a trait/structure param **optional** for conformers | **grammar-fixture + wired-on-main**: `undef-optional.ri` — conformer to `ImpactResistant{charpy_impact:Real=undef izod_impact:Real=undef}` supplying only `charpy_impact` ⇒ "All constraints satisfied". Grammar: `param_declaration` `= binding_value` (grammar.js:578-585) + `undef_literal` (grammar.js:1488, task #3918, commit `856f1dd711`). Conformance: `param`-default-satisfies-`param`-requirement (`conformance/checker.rs:3-12`). | **PASS** |
| 2 | α/β | omitting a **non-defaulted** param still errors (optionality is genuine, not a no-op) | **negative control**: `required-fail.ri` — omitting `izod_impact : Real` (no default) ⇒ `error: missing required member 'izod_impact'`. | **PASS** (gap real) |
| 3 | α | chained `constraint 0 < poissons_ratio < 0.5` parses **and enforces both bounds** | **grammar-fixture + numeric floor**: `chained-constraint.ri` ⇒ `0.3` `OK`; `chained-violate-hi.ri` `0.7` ⇒ `VIOLATED … error`; `chained-violate-lo.ri` `-0.1` ⇒ `VIOLATED`. Not mis-parsed as `(0<x)<0.5`. | **PASS** |
| 4 | α | new `Elastic` constraint breaks **no existing conformer** | **numeric floor / corpus sweep**: every `poissons_ratio` literal in `examples/`+`crates/` is `0.29`/`0.3`/`0.33` ∈ (0,0.5). Doc bound deliberately excludes auxetic (ν<0) + incompressible (ν=0.5) — intended exclusion, not a guessed tolerance. | **PASS** |
| 5 | α | `Temperature` alias + dimensioned default `293.15K`, and `Int` param type | **grammar-fixture**: `temp-default.ri` — `param reference_temperature : Temperature = 293.15K`, conformer omits ⇒ clean, supplies `350.0K` ⇒ clean (even under `#no_prelude`). `int-type.ri` — `param fatigue_cycles : Int = undef`, conformer supplies `1000000` ⇒ clean. | **PASS** |
| 6 | β | renames break **only tests + examples** (anti-hidden-consumer) | **wired-on-main / producer-absent check**: `grep -nE '\b(uts\|elongation\|impact_energy\|endurance_limit)\b'` over non-test `crates/` finds **no** solver/eval/kernel reader. Consumers = `m8_materials.ri`, `io_export.ri`, `drivebelt_trait_bounds.ri`, `materials_mechanical_tests.rs`, `stdlib_loader_tests.rs`, `m8_3_stdlib_integration.rs`, `parametric_tensor_resolution_tests.rs`, `cross_module_alias_propagation_tests.rs`, `drivebelt_trait_bounds.rs`, `reify-test-support/src/fixtures.rs`. All in β's file set ⇒ atomic migration, main stays green. | **PASS** |
| 7 | β | renamed names declarable; old names removed (anti-mismatch) | **grammar-fixture**: a `.ri` declaring `Strong{ultimate_tensile_strength}`/`Ductile{elongation_at_break}`/`FatigueRated{fatigue_limit,fatigue_strength_at,fatigue_cycles}`/`ImpactResistant{charpy_impact,izod_impact}` parses + checks (all are existing grammar shapes — `param name : Type[= undef]` + `constraint a >= b`). Post-β, `uts`/`elongation`/`endurance_limit`/`impact_energy` resolve to `unresolved`/`missing required member`. | **PASS** |
| 8 | β | integration: `examples_smoke.rs` + drivebelt test consume the migrated surface (anti-orphan gate) | **wired-on-main**: `crates/reify-compiler/tests/examples_smoke.rs` walks `examples/*.ri` and compiles each (drivebelt header self-documents this); `crates/reify-eval/tests/drivebelt_trait_bounds.rs` is the dedicated integration test. β's migration is verified by these existing CI gates — the C-as-integration-gate for the breaking change. | **PASS** |
| 9 | γ | thermal/optical optionality additive (no breakage); Refractory constraint degrades gracefully | **producer-dep / numeric floor**: making required→optional is a relaxation (existing setters stay valid). `Refractory`'s `max_service_temperature >= 1500.0` over an omitted (`undef`) value ⇒ INDETERMINATE warning, not error (Kleene rule, arch §2.5 — same mechanism proven in row 11). | **PASS** |
| 10 | δ | sub-trait CANNOT re-require an optional parent param | **grammar-fixture (negative)**: `subtrait-rerequire.ri` — re-declaring `dielectric_strength : Real` (no default) in `Insulating : ElectricallyCharacterized{… = undef}` does **not** force a conformer to supply it (`OmitsDielectric` passes). ⇒ D4 (optional-everywhere) is the only coherent behavior; no false "re-require" premise. | **PASS** |
| 11 | δ | omitting `dielectric_strength` ⇒ Insulating `> 0` constraint = INDETERMINATE **warning** (premise of D4 true, not aspirational) | **field-population / numeric floor**: `subtrait-rerequire.ri` `OmitsDielectric` ⇒ `INDETERMINATE OmitsDielectric#constraint[1]` + `warning: constraint … indeterminate: undefined inputs` + `No constraints violated (1 indeterminate)` exit 0; `GoodInsulator` (supplies `2.0e7`) ⇒ `OK`. Consistent with #2484's accepted weakening. | **PASS** |
| 12 | ε | reconciled doc forms match shipped API (anti-mismatch) | **grammar-fixture**: post-batch, every §6 form (`MaterialSpec` base; free-standing `Elastic`/`Strong`/`Hard`/`Ductile`; restored param names; `dielectric_strength > 0`; no `determined(...)`; no base `trait Material`) is a verified-parseable shape from rows 1/3/5/7. Dimensioned types left as the #3111-family target (not downgraded). | **PASS** |

**Cross-PRD ordering ledger (G4, mechanizes §6).** Producer-side dependency
edges, **not** orphan-consumer risks:
- `3111 → β` — #3111 (deferred, Real→`Pressure`/`Energy`, mechanical) must retype
  the **renamed** params; its title still names `uts`/`endurance_limit`/
  `impact_energy` (stale post-β) — details annotated with the rename map.
- `3112 → γ` / `3113 → γ` — both edit `materials_thermal.ri` /
  `materials_optical.ri`; ordered after γ to avoid same-file rebase. `= undef` is
  type-agnostic, so γ's optionality and their retyping compose.

**Substrate-staleness ledger (the false premise this PRD corrects).** The
gap-register P12 "Documented `= undef` optional params are required in .ri" row
and the `materials_electrical.ri` Decision-#3 / `io.ri` lines 11–12 comments all
assert "the Reify grammar has no `undef` keyword." **False as of task #3918**
(commit `856f1dd711`, `undef_literal` grammar rule + optional-param conformance).
Rows 1/2 are the empirical refutation; δ/ε scrub the stale comment text.
