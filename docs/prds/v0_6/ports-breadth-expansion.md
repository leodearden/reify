# PRD: `std.ports` Breadth Expansion — mechanical/electrical/thermal/fluid port traits

Status: **author-complete** (interactive `/prd` session, 2026-06-02). Cluster `ports-breadth-2026-06-02`, milestone `v0_6`. Closes the **P11 ports-breadth** row-group of `docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md` (4 gaps, 1 high) plus the five §5 Bucket-B "dropped-param" reconciliations the gap register lists as untracked-by-any-open-task.

Approach **B** (vertical slice, per-domain task + integration-gate leaf). Confirmed 2026-06-02: this is a declarative trait-surface expansion over **done** foundations (4022/4023/4027), the only cross-crate consumer (`connect.rs`) already exists, and no grammar/parser/FEA/dispatch seam is touched.

## §0 — Goal and user-observable surface

Tasks **4022/4023** authored the *minimal* `std.ports` surface deliberately (empty markers + a defensible param subset; PRD `stdlib-reconstruction.md` §8 Q3/Q4). This PRD is the **breadth expansion** the spec §5 promises and the gap register flags. After it lands, a designer can write, and the compiler accepts / the evaluator computes:

```
import std.ports.mechanical

// A threaded fastener port carrying a full standards-derived thread spec.
structure def BoltHole {
    port thread : in ThreadedPort {
        param thread_spec : ThreadSpec = ThreadSpec(
            system: ThreadSystem.ISO_Metric,
            nominal_diameter: 6mm,
            pitch: 1mm,
            thread_class: ThreadClass.Class_6g6H,
        )
    }
}
// reify eval → thread.thread_spec.minor_diameter == 4.9175mm
//              thread.thread_spec.pitch_diameter == 5.3505mm
//              thread.thread_spec.tap_drill      == 5mm
```

Concretely, after this PRD lands:

- **Base ports gain spatial identity.** A `LocatedPort` trait exists in the stdlib (the compiler has expected it *by name* since task 370 — `reify-core/src/identity.rs:115 LOCATED_PORT_TRAIT`), a `RegionPort` refines it with a `region : Geometry`, `Port.direction` carries its documented `= Directionality.Bidi` default, and a `Frame3` structure makes port frames constructible. Connecting a located port to a non-located one now fires the existing `connect.rs` **asymmetric-LocatedPort** warning (built by done task 370, previously unreachable from stdlib-derived ports).
- **Mechanical gains the full thread + motive/guide hierarchy.** `ThreadSpec` (with three enums `ThreadSystem`/`ThreadClass`/`ThreadTighteningDirection` and standards-derived `let`s `minor_diameter`/`pitch_diameter`/`tap_drill`/`clearance_hole`), `MotivePort`/`RotaryPort`/`LinearPort`, and `GuidePort`/`LinearGuidePort`/`RotaryGuidePort` (with the `degrees_of_freedom == 1` constraints).
- **Electrical regains dropped params.** `ElectricalPort.voltage_rating`/`current_rating`, `SignalKind.PWM`, and `PinPort` (a spatially-located pin).
- **Thermal regains dropped params.** `ThermalPort.heat_flux`/`thermal_resistance` and `ThermalContactPort` (a region-located thermal interface).
- **Fluid regains its categorical + piping surface.** `FluidType` enum + `fluid_type`, `PipedFluidPort`, `PipeConnectionType`, and the §5.6 multi-domain `HydraulicPort` example.

**User-observable surfaces (G1/G2):**
1. **Compiler diagnostics** — the existing `W` asymmetric-`LocatedPort` warning (`connect.rs:344`) fires for stdlib-derived located ports; a `LinearGuidePort` with `degrees_of_freedom ≠ 1` raises a constraint violation.
2. **`reify eval`** — `ThreadSpec(...)` constructs at runtime (GR-001 done) and its derived `let`s evaluate to standards-table values (observable exactly as `AluminumBracket.mass` is in `m8_3_stdlib_integration.rs`).
3. **`reify check`** — the new traits/enums resolve as port-type annotations and trait bounds (previously `Undef` / unresolved: `SignalKind.PWM`, `RegionPort`, `LocatedPort`).
4. **CI `.ri` examples** under `examples/stdlib/` that import each submodule and exercise the new surface, validated by the `stdlib_loader` growing-prelude tests and the `m8_*_stdlib_integration` eval tests.

## §1 — Background and provenance

Spec §5 (`docs/reify-stdlib-reference.md:579-731`) defines the full port trait surface. The 13-agent stdlib survey (gap register P11) found the shipped `.ri` (tasks 4022/4023, all done) deliberately minimized it:

| Gap | Sev | Shipped today | This PRD |
|---|---|---|---|
| `ThreadSpec`/`ThreadSystem`/`ThreadClass` + standards-derived `let`s | **high** | `ThreadedPort` has only `thread_diameter`/`pitch` raw `Length` | full `ThreadSpec` structure + 3 enums + derived `let`s |
| `LocatedPort` absent from stdlib though compiler expects it by name | **high** | only the `connect.rs` consumer exists (task 370) | declare `LocatedPort : Port { frame : Frame3 }` |
| `RegionPort` missing | med | absent | `RegionPort : LocatedPort { region : Geometry }` |
| `MotivePort`/`LinearPort`/`GuidePort`/`LinearGuidePort`/`RotaryGuidePort` | med | only `RotaryPort` (renamed params) | full motive/guide hierarchy |
| electrical: voltage/current ratings + `PinPort` + `SignalType.PWM` | (B) | `ElectricalPort {}` empty; `SignalKind` no PWM; no `PinPort` | restore all three |
| thermal: `heat_flux`/`thermal_resistance` + `ThermalContactPort` | (B) | `ThermalPort {temperature, heat_flow}` only | restore + add contact port |
| fluid: `FluidType`/`PipedFluidPort`/`PipeConnectionType` | (B) | `FluidPort {pressure, flow_rate, medium}` | restore enum + piped port + multi-domain |
| `Port.direction` default `Bidi` | (B) | no default | add `= Directionality.Bidi` |

The Bucket-B rows are flagged in the register as "shipped by a done task, restoration untracked by any open task" — they are **in scope here** as the deliberate breadth pass, not doc-reconcile. The one true doc-reconcile is `SignalType → SignalKind` (the doc is stale): we keep the shipped name `SignalKind` and **add** the missing `PWM` variant.

## §2 — Sketch of approach

Five `.ri` files under `crates/reify-compiler/stdlib/` (`ports.ri`, `ports_mechanical.ri`, `ports_electrical.ri`, `ports_thermal.ri`, `ports_fluid.ri`) are **edited in place** — all five are already registered in `stdlib_loader.rs` (tasks 4022/4023), so no loader change is needed. The growing-prelude load order (`ports → mechanical/electrical/thermal/fluid`) means submodules may refine base traits, and `ports_fluid` (last) may refine `MechanicalPort` for `HydraulicPort`.

One small **compiler substrate** change is required (slice **G**): the doc types `region`/`thread_form` as `Geometry`, but `type_resolution.rs` only blesses `Solid` (→ `Type::Geometry`). Per the "one `Type::Geometry`, refinement-by-marker-trait" decision (`decisions_geometry_traits_not_types`), we add the canonical `"Geometry" => Type::Geometry` arm (mirroring the existing `"Solid"` arm at `type_resolution.rs:563`).

**Spatial-type encoding (resolved design decision).** The doc's `Frame<3>` and `Axis` have **no surface type** — `Frame` is internal-only (`dynamics.ri:62-64`: "`Frame` is absent from type_resolution.rs — Type::Frame is internal-only"), and the geometry `Axis` value type is unconstructible from source (owned by the P6 geometry-frames-constructors cluster). We follow the established `constitutive.ri` convention ("axes are stored as `Vector3<Length>`"):
- `LocatedPort.frame : Frame3`, where **`Frame3` is a new `structure def`** (`origin` + `x_axis`/`y_axis`/`z_axis : Vector3<Length>`) mirroring `constitutive.ri`'s `MaterialFrame`. Constructible, gives honest spatial semantics, and the `connect.rs` consumer is name/refinement-based (never reads the frame value), so this satisfies it immediately.
- Motive-port `axis : Vector3<Length>` (the rotation/translation axis direction), **not** the unconstructible geometry `Axis` type.

**Optional params (resolved).** The doc's `= undef` defaults are **grammar-fiction** — authoritatively confirmed by `io.ri:11-12` and `materials_electrical.ri:24` ("The Reify grammar has no `undef` keyword"). Optional params become `Option<T> = none` (precedent: `fdm_correlations.ri:242`, `materials_fea.ri:136`); required params carry no default.

**Standards-derived `let`s (resolved; G6).** `if`/`match` do **not** parse in a `let` RHS (verified), so derived `let`s are pure arithmetic over params. For all four `ThreadSystem` variants (all 60°-flank threads), the ISO 68-1 / ISO 261 geometric identities hold exactly:
- `minor_diameter = nominal_diameter - 1.0825 * pitch` (= `D - 1.25H`, `H = 0.866P`) — M6×1 → 4.9175 mm (ISO d1 = 4.917 mm). **exact**.
- `pitch_diameter = nominal_diameter - 0.6495 * pitch` (= `D - 0.75H`) — M6×1 → 5.3505 mm (ISO d2 = 5.350 mm). **exact**.
- `tap_drill = nominal_diameter - pitch` — M6×1 → 5.0 mm (standard ~75% tap drill). **exact for the rule**.
- `clearance_hole = nominal_diameter + 0.5 * pitch` — M6×1 → 6.5 mm, a **documented approximation** of the ISO 273 medium fit (6.6 mm). The fit-class table (close/medium/coarse) requires per-class branching that the `let` grammar can't express; deferred (see §7 + §8 Open questions).

**Refinement-chain restructure.** Today `MechanicalPort : Port`. The doc has `MechanicalPort : LocatedPort` (a mechanical port is spatially located). Slice β restructures this so mechanical/threaded/motive/guide ports transitively satisfy `LocatedPort` — which is precisely what makes the `connect.rs` asymmetric warning fire for them.

## §3 — Pre-conditions for activating

- **GR-001 (structure-instance runtime) — DONE** (2026-05-26, `Value::StructureInstance`). Required for the `ThreadSpec(...)` eval signal. ✔
- **Slice G (Geometry surface alias)** — in-batch prerequisite; α and β depend on it (`region : Geometry`, `thread_form : Option<Geometry>`). Filed in this batch, wired as a hard dep.
- Tasks **4022/4023/4027** (minimal ports + prelude re-export) — **done**; this PRD edits their files. ✔
- No grammar prerequisite: all in-scope syntax parses with 0 ERROR nodes (§G3 below).

## §4 — Resolved design decisions

1. **`frame : Frame3`** (new structure def, `Vector3<Length>` members) — not `Frame<3>` (no surface type), not `Real` placeholder. *(decided 2026-06-02)*
2. **`region`/`thread_form : Geometry`** via a new canonical `Geometry` surface alias in `type_resolution.rs` — not the legacy `Solid` spelling. *(decided 2026-06-02)*
3. **Motive `axis : Vector3<Length>`** — not the unconstructible geometry `Axis` type (P6-owned).
4. **`SignalKind` retained + `PWM` added** — the `SignalType→SignalKind` rename is doc-reconcile (doc is stale); only PWM is a real restoration.
5. **Optional params → `Option<T> = none`**; required params carry no default. `Port.direction = Directionality.Bidi` (enum-variant default; precedent `solver_buckling.ri:101`).
6. **`thread_spec : ThreadSpec` replaces** `ThreadedPort`'s raw `thread_diameter`/`pitch` (no real external conformers — `m8_ports.ri` uses an independent inline `ThreadedPort`).
7. **`fluid_type : FluidType` is additive** — keep the shipped `medium : String` (free-form) alongside the new categorical enum.
8. **Approach B** (vertical slice + integration-gate leaf), not B+H.

## §5 — Out of scope

- A general `Frame3`/`Axis`/`Plane` **geometry surface-type + constructor** family — owned by the P6 geometry-frames-planes-constructors cluster (`reference_prd_geometry_transforms_frames_projection`). This PRD's `Frame3` is a local port-frame structure; consolidation with P6 is a future merge.
- **Thread fit-class table** (ISO 273 close/medium/coarse selection) — `clearance_hole` ships as a documented single-formula approximation; per-class branching is deferred (needs `if`/`match` in `let` RHS or a `fit` param + dispatch).
- **`FluidPort.pressure_range : Range<Pressure>`** — the doc's `Range<T>` form is unrealizable (task 4023 already replaced it with scalar `pressure`/`flow_rate`); not re-litigated here.
- **`thread_form` geometry generation** — `thread_form : Option<Geometry> = none` is a carrier param only; no thread-helix kernel op.
- Compile-time enforcement that a `port x : T` requires `T : Port` (task 4027 §"Out of scope"; separate compiler-check fork).

## §6 — Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/stdlib-reconstruction.md` | extends | the 5 `ports*.ri` files + `Port`/`Directionality`/`SignalKind` | **this-prd** | the minimal versions are done (4022/4023); breadth is this PRD's |
| geometry-frames-planes-constructors (P6) | defers-to | a shared `Frame3`/`Axis` surface type + constructors | other-prd | this PRD uses local `Frame3` + `Vector3<Length>` axes to avoid a hard dep; future consolidation |
| `connect.rs` (compiler, task 370 done) | produces-for | `trait_satisfies(LOCATED_PORT_TRAIT)` asymmetric warning + `Directionality` compatibility | **this-prd** (producer of the trait) | consumer wired-on-main; this PRD ships the `LocatedPort` it detects |

No contested-ownership seam (none of the three known pairs in the overlay G4 list is touched). The `Geometry` surface alias is additive (one arm), not contested.

## §7 — Decomposition plan (one bullet per task, naming its observable signal)

Each slice edits a **distinct file** (no narrow-file-lock contention). α/β/δ/ε/ζ are intermediates (η consumes them); each also carries a per-slice compile-observable signal via the `stdlib_loader` growing-prelude tests + a focused `examples/stdlib/ports_<domain>.ri` fixture. **η is the integration-gate leaf** (C-as-integration-gate) holding the comprehensive eval signal.

- **G — Geometry surface-type alias.** Add `"Geometry" => Some(Type::Geometry)` to `resolve_type_name` (`crates/reify-compiler/src/type_resolution.rs`, beside `"Solid"`).
  *Signal:* a `type_resolution` unit test asserts `resolve_type_name("Geometry") == Some(Type::Geometry)` (mirror the `"Solid"`→Geometry test at :2117); `reify check` accepts a `.ri` with `param r : Geometry`. *Consumer:* α, β.
- **α — base: `Frame3`, `LocatedPort`, `RegionPort`, `Bidi` default.** Edit `ports.ri`: add `structure def Frame3`; `Port.direction = Directionality.Bidi`; `trait LocatedPort : Port { param frame : Frame3 }`; `trait RegionPort : LocatedPort { param region : Geometry }`.
  *Signal:* `stdlib_loader` tests compile `ports.ri` in the growing prelude; `reify check` on a fixture connecting a `LocatedPort`-derived port to a non-located port emits the existing `W` asymmetric-LocatedPort warning (`connect.rs:344`); a `Port` with no explicit `direction` defaults to `Bidi` (compatibility passes). *Consumer:* β/δ/ε/ζ/η. *Depends:* G.
- **β — mechanical: `ThreadSpec` + motive/guide hierarchy.** Edit `ports_mechanical.ri`: `MechanicalPort : LocatedPort` (restructure) + `max_load`/`max_torque : Option<…>`; `ThreadSpec` structure + derived `let`s + `ThreadSystem`/`ThreadClass`/`ThreadTighteningDirection`; `ThreadedPort.thread_spec : ThreadSpec`; `MotivePort`, `RotaryPort` (reconcile `torque_capacity→max_torque`, add `axis`), `LinearPort`, `GuidePort`, `LinearGuidePort`, `RotaryGuidePort`.
  *Signal:* `reify eval` on a fixture constructing `ThreadSpec(system: ISO_Metric, nominal_diameter: 6mm, pitch: 1mm, …)` reads `minor_diameter == 4.9175mm`, `pitch_diameter == 5.3505mm`, `tap_drill == 5mm`; `reify check` rejects a `LinearGuidePort` with `degrees_of_freedom == 2` (constraint violation). *Consumer:* ζ, η. *Depends:* α, G.
- **δ — electrical breadth.** Edit `ports_electrical.ri`: `ElectricalPort { voltage_rating : Voltage, current_rating : Current }`; `PowerPort.power_rating : Power`; add `PWM` to `SignalKind`; `PinPort : ElectricalPort + LocatedPort { pin_id : String }`.
  *Signal:* `reify check` resolves `SignalKind.PWM` (previously `Undef`) and accepts a `PinPort` with `pin_id`; `reify eval` of a `PowerPort(voltage_rating: 12V, current_rating: 5A)` yields those cells. *Consumer:* η. *Depends:* α.
- **ε — thermal breadth.** Edit `ports_thermal.ri`: `pub type HeatFlux = Power / Area`, `pub type ThermalResistance = Temperature / Power`; `ThermalPort.heat_flux`/`thermal_resistance : Option<…>`; `ThermalContactPort : ThermalPort + RegionPort { contact_area : Area, contact_conductance : Option<ThermalConductivity> }`.
  *Signal:* `reify check` accepts a `ThermalContactPort` with `heat_flux : HeatFlux` + `region : Geometry` (both alias + RegionPort resolve in param position); `stdlib_loader` tests compile the module. *Consumer:* η. *Depends:* α.
- **ζ — fluid breadth + multi-domain.** Edit `ports_fluid.ri`: `enum FluidType` + `FluidPort.fluid_type`; `enum PipeConnectionType`; `PipedFluidPort : FluidPort + LocatedPort { inner_diameter, connection_type }`; `enum FittingStandard` + `HydraulicPort : FluidPort + MechanicalPort { fitting_type }` (§5.6).
  *Signal:* `reify check` accepts `PipedFluidPort` (`connection_type: PipeConnectionType.Threaded`) and the multi-domain `HydraulicPort`; `FluidType.Liquid` resolves. *Consumer:* η. *Depends:* α, β (HydraulicPort refines MechanicalPort).
- **η — integration gate (leaf).** Add `examples/stdlib/ports_breadth.ri` (a multi-domain assembly importing all submodules) + assertions in `crates/reify-eval/tests/` (extend the `m8_ports`/`m8_3_stdlib_integration` pattern).
  *Signal:* one integration test asserts, end-to-end: (a) the `ThreadSpec` derived-let numerics above via `reify eval`; (b) the `connect.rs` asymmetric-LocatedPort warning fires for an asymmetric mechanical↔non-located connection; (c) a `HydraulicPort` conforms (`FluidPort + MechanicalPort` multi-domain). *Consumer:* CI / the designer. *Depends:* α, β, δ, ε, ζ.

DAG: `G → α → {β, δ, ε}`; `β → ζ`; `{α,β,δ,ε,ζ} → η`.

## §8 — Open questions (tactical)

- **`clearance_hole` fit class.** Ships as the medium-fit approximation `D + 0.5P`. A future `param fit : HoleFit = HoleFit.Medium` with per-class values needs either `if`/`match` in `let` RHS (grammar work) or a builtin lookup — file a follow-up if designers need close/coarse.
- **`Frame3` home.** Local to `ports.ri` for now; if P6 lands a shared geometry `Frame3`/`Axis` surface type, migrate `LocatedPort.frame` and motive `axis` to it (additive, non-breaking if field-compatible).
- **Eval-time sub re-declaration.** Per the `m8_tolerancing.ri` workaround note, the η example may need to locally re-declare `ThreadSpec` for eval-time sub resolution (the known `compile_with_stdlib` template-export gap) — implementer's choice; mirror the established pattern.
- **`thread_class` vs doc `class`.** The doc uses `param class`; `class` parses as a plain identifier, but `thread_class` is used here to avoid any soft-keyword ambiguity in member access (`spec.thread_class`). Tactical rename; revisit if doc-fidelity is preferred.
