# Capability Manifest — `ports-breadth-expansion`

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/ports-breadth-expansion.md`. Every binding below is PASS or has a named upstream producer; **no FAIL binding** blocks the batch.

Evidence conventions (reify overlay): empty-value sentinel = `Value::Undef`/`None`; production entry paths = `stdlib_loader` growing-prelude + `type_resolution.rs` + `reify-eval` eval of `Value::StructureInstance`; grammar-fixtures parsed with `tree-sitter parse --quiet` (0 ERROR nodes) under `/tmp/prd-gate-fixtures/`.

---

## G — Geometry surface-type alias

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `resolve_type_name("Geometry") → Type::Geometry` | capability→producer | new arm beside `type_resolution.rs:563 "Solid" => Type::Geometry`; this slice IS the producer | PASS (this task) |
| `Type::Geometry` exists | wired-on-main | `reify_core::Type::Geometry` referenced throughout `geometry_traits_inference.rs`, `type_compat.rs:48` | `grep:crates/reify-compiler/src/type_resolution.rs:563 wired` |
| test asserts resolution | grammar/test | mirror existing `"Solid" should resolve to Type::Geometry` test at `type_resolution.rs:2117-2118` | PASS |

## α — base: Frame3 / LocatedPort / RegionPort / Bidi default

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `structure def Frame3 { … : Vector3<Length> }` | grammar-fixture | `/tmp/prd-gate-fixtures/final-base.ri` parses 0 ERROR; `Vector3<Length>` resolves — `constitutive.ri:55` precedent | PASS |
| `param frame : Frame3` (structure-typed param) | wired-on-main | structure-typed params resolve via `type_resolution.rs:636 StructureRef`; precedent `constitutive.ri:157 param frame : MaterialFrame` | `grep:crates/reify-compiler/src/type_resolution.rs:636 wired` |
| `param region : Geometry` | capability→producer | resolves once slice **G** lands | `producer:task-G upstream` |
| `Port.direction = Directionality.Bidi` (enum-variant default) | grammar-fixture | `final-base.ri` parses; semantic precedent `solver_buckling.ri:101 param element_order : ElementOrder = ElementOrder.P1` | PASS |
| `LocatedPort` consumed (asymmetric warning) | capability→producer (anti-orphan) | `connect.rs:322 trait_satisfies(lt, LOCATED_PORT_TRAIT, …)` + `:344` warning — wired on main (task 370 done) | `grep:crates/reify-compiler/src/connect.rs:322 wired` |
| `trait X : Y` requires `{}` body | grammar reality | bodiless `trait MotivePort : MechanicalPort` FAILs; braced `{}` PASSes — author with `{}` | noted (PASS with `{}`) |

## β — mechanical: ThreadSpec + motive/guide

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `ThreadSpec` structure + `let`s + 3 enums + multi-param body | grammar-fixture | `/tmp/prd-gate-fixtures/final-mech.ri` parses 0 ERROR | PASS |
| `ThreadSpec(...)` constructs at runtime | wired-on-main (GR-001) | `Value::StructureInstance` shipped 2026-05-26 (gap-register GR-001 DONE); `reify-eval/src/lib.rs:274` StructureInstance arm | `grep:crates/reify-eval/src/lib.rs:274 wired` |
| derived `let` readable off instance via `reify eval` | field-population | derived-let eval observable — precedent: `m8_3_stdlib_integration.rs` reads `AluminumBracket.mass` (a `let mass = …`); `structural_physical.ri:40` | `grep:crates/reify-eval/tests/m8_3_stdlib_integration.rs (AluminumBracket.mass) wired` |
| `minor_diameter = D − 1.0825·P` exact | numeric floor (G6-2 closed-form) | ISO 68-1 identity `D − 1.25H, H=0.866P`; M6×1 → 4.9175 mm vs ISO d1 4.917 mm; config = 60°-flank thread (all 4 ThreadSystem variants) | PASS (exact) |
| `pitch_diameter = D − 0.6495·P` exact | numeric floor (G6-2) | ISO `D − 0.75H`; M6×1 → 5.3505 mm vs ISO d2 5.350 mm | PASS (exact) |
| `tap_drill = D − P` | numeric floor (G6-2) | standard ~75%-engagement tap-drill rule; M6×1 → 5.0 mm | PASS (exact for rule) |
| `clearance_hole = D + 0.5·P` | numeric (G6-1, **approximation**) | documented medium-fit approx of ISO 273 (M6 → 6.5 vs 6.6 mm); marked provisional, fit-class deferred (§5/§8) | PASS (provisional, labeled) |
| `Torque` alias in param position | grammar-fixture | `pub type Torque = Force * Length / Angle` — shipped `ports_mechanical.ri:28` | PASS |
| motive `axis : Vector3<Length>` | wired-on-main | `constitutive.ri:39-40` "axes stored as Vector3<Length>"; `fdm.ri` build_direction | `grep:crates/reify-compiler/stdlib/constitutive.ri:55 wired` |
| `constraint degrees_of_freedom == 1` in trait body | grammar-fixture | `final-mech.ri` parses; precedent `Elastic` constraint in trait body (spec §6.2) | PASS |
| `thread_form : Option<Geometry> = none` | capability→producer | `Option<T>=none` shipped (`fdm_correlations.ri:242`); `Geometry` via slice **G** | `producer:task-G upstream` |

## δ — electrical breadth

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `Voltage`/`Current`/`Power` in param position | wired-on-main | NAMED_DIMENSIONS `dimension.rs:476,488,490` | `grep:crates/reify-core/src/dimension.rs:476 wired` |
| `SignalKind.PWM` resolves | field-population | adding a variant to the shipped `enum SignalKind` (`ports_electrical.ri:26`); enum-variant resolution is standard | PASS |
| `PinPort : ElectricalPort + LocatedPort` (multi-supertrait) | grammar-fixture | `/tmp/prd-gate-fixtures/final-elec.ri` + `ports-01-multi-refine.ri` parse 0 ERROR | PASS |
| `LocatedPort` available | capability→producer | refines α's trait | `producer:task-α upstream` |

## ε — thermal breadth

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `pub type HeatFlux = Power / Area`, `ThermalResistance = Temperature / Power` | grammar-fixture | `/tmp/prd-gate-fixtures/final-therm.ri` parses; alias-on-RHS precedent `Torque`/`VolumetricFlowRate` (`ports_fluid.ri:31`) | PASS |
| `HeatFlux` absent from NAMED_DIMENSIONS → alias needed | substrate check | `rg HeatFlux dimension.rs` = 0 hits → alias indirection required (confirmed) | PASS (alias) |
| `ThermalConductivity` in param position | wired-on-main | NAMED_DIMENSIONS `dimension.rs:511` | `grep:crates/reify-core/src/dimension.rs:511 wired` |
| `ThermalContactPort : ThermalPort + RegionPort` | grammar-fixture + producer | multi-refine parses; `RegionPort` from α | PASS / `producer:task-α upstream` |

## ζ — fluid breadth + multi-domain

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `FluidType`/`PipeConnectionType`/`FittingStandard` enums | grammar-fixture | `/tmp/prd-gate-fixtures/final-fluid.ri` parses 0 ERROR | PASS |
| `PipedFluidPort : FluidPort + LocatedPort` | grammar/producer | multi-refine parses; `FluidPort` shipped (`ports_fluid.ri:40`), `LocatedPort` from α | PASS / `producer:task-α upstream` |
| `HydraulicPort : FluidPort + MechanicalPort` | DAG-direction | `MechanicalPort` restructured in β (upstream); load order `fluid` last → both visible | `producer:task-β upstream` |

## η — integration gate (leaf)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| eval `ThreadSpec` derived-let numerics | field-population | β delivers + GR-001 eval path; test pattern = `m8_3_stdlib_integration.rs` | `producer:task-β upstream` |
| `connect.rs` asymmetric-LocatedPort warning fires | capability→producer | consumer wired on main (task 370); α ships the located trait | `grep:crates/reify-compiler/src/connect.rs:344 wired` + `producer:task-α upstream` |
| `HydraulicPort` multi-domain conformance | DAG-direction | ζ delivers (depends β) | `producer:task-ζ upstream` |
| example/test files distinct from slices | anti-conflict | η owns `examples/stdlib/ports_breadth.ri` + `reify-eval/tests/` additions; per-slice fixtures are distinct files | PASS |

---

**Summary:** 0 FAIL bindings. The only non-`.ri` substrate is the `Geometry` alias (slice **G**, in-batch upstream of α/β). All grammar fragments parse 0-ERROR. The single non-exact numeric (`clearance_hole`) is explicitly labeled an approximation with the fit-class refinement deferred (§5/§8) — the η eval assertions are on the **exact** lets (`minor_diameter`/`pitch_diameter`/`tap_drill`).
