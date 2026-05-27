# PRD: Stdlib Reconstruction — `std.ports`, `std.process`, `std.fields`, `std.units` constants

Status: **deferred** (Bucket-3 lost-work reconstruction). Authored 2026-05-27 in an interactive `/prd` session, batch `spec-gap-2026-05-27`, cluster `stdlib-reconstruction`. Orchestrator stopped; this PRD's tasks are filed `deferred` and NOT flipped to pending.

Recovers four stdlib `.ri` module surfaces that the language spec §11 promises and whose authoring tasks are marked **done**, but whose `.ri` files are **absent on disk** — lost to the DB-rebuild / recycled-ID churn that this spec-gap survey exists to clean up. This is the same failure shape as task #2443 (which re-authored `io.ri` after task #339 phantom-done) and the io-module precedent guides the whole PRD.

## §0 — Goal and user-observable surface

A designer can write, and the compiler accepts:

```
import std.ports.mechanical
import std.process
import std.fields

// a coupling parameterised over two rotary ports (spec §6 example, line 494)
structure def FlexibleCoupling<DriverPort: RotaryPort, DrivenPort: RotaryPort> {
    port driver : DriverPort
    port driven : DrivenPort
}

// a manufacturing-cost rollup using the Process trait hierarchy
let machining_cost = some_subtracting_process.cost

// a physical constant in a body-force load
let thermal_energy = BOLTZMANN_CONSTANT() * 300K
```

Concretely, after this PRD lands:
- `import std.ports.mechanical` (and `.electrical` / `.thermal` / `.fluid`) compiles; `RotaryPort`, `PowerPort`, `SignalPort`, `ThreadedPort`, `Bore`, `Shaft` etc. are usable as port-type annotations and as trait bounds.
- `import std.process` compiles; `Process`, `Subtracting`, `Adding`, `Forming`, `Joining`, and `DFMRule` are usable as trait bounds.
- `import std.fields` compiles; it packages the existing built-in differential operators (`gradient`, `divergence`, `curl`, `laplacian`, `sample`) behind a documented module surface, and exposes the `Field<D,C>` type alias and field-construction helpers.
- `SPEED_OF_LIGHT()` and `BOLTZMANN_CONSTANT()` join `STANDARD_GRAVITY()` in `std.units`, completing the spec §11.3 promise of `pi`/`g`/`c`/`boltzmann`.

Each is a stdlib `.ri` authoring task: append the `.ri` file under `crates/reify-compiler/stdlib/`, register it in `crates/reify-compiler/src/stdlib_loader.rs`, and verify with a CI example `.ri` that imports the module and exercises one trait / constant. The language already supports `trait`, `structure def`, `enum`, `pub fn`, and `pub type` — so **grammar_confirmed = true** for all four slices (verified, §G3 below).

## §1 — Background and provenance

Spec §11.1 (module tree) and §11.3 (module summaries) promise this surface:

| Module | Spec §11.3 promise | On-disk state (2026-05-27) |
|---|---|---|
| `std.ports` | Port trait hierarchy: mechanical (`Bore`, `Shaft`, `RotaryPort`), electrical (`PowerPort`, `SignalPort`), thermal, fluid. Directionality + compatibility rules. | **ABSENT.** No `ports*.ri`. Done tasks #336 (integration test referencing `ports.ri`) and #333 mention it. |
| `std.process` | Manufacturing process traits (`Subtracting`, `Adding`, `Forming`, `Joining`), DFM rule framework. | **ABSENT.** No `process.ri`. Done task #333 ("Stdlib: Process traits + DFMRule"). |
| `std.fields` | Field interpolation, spatial ops (`compose`, `sample`, `restrict`), differential operators (`gradient`, `divergence`, `curl`, `laplacian`). | **ABSENT as a module.** No `fields*.ri`. Done task #344 references `fields_analysis.ri`. **The operators themselves ARE built in** (`reify-expr/src/calculus.rs`, dispatched in `reify-expr/src/lib.rs:198-349`). The gap is the `.ri` module surface that packages them. |
| `std.units` constants | `pi`, `g`, `c`, `boltzmann`. | **PARTIAL.** `units.ri:82` has `STANDARD_GRAVITY()` (= `g`); `pi` is a compiler builtin (`constants.rs`). `c` and `boltzmann` are **missing**. |

The established remediation precedent is task #2443 ("Author stdlib io.ri … Task 339 was marked done without authoring the file"). `io.ri` is the authoring template: a `#no_prelude` header, marker + refining traits, enums, and an explicit `// Deviations from the §N spec` comment block where current language scope forces a divergence from the stdlib reference.

### What this PRD deliberately does NOT touch — the OpenVDB imported-field front-end

The survey brief lists the OpenVDB `field def … imported` front-end (deferred at `crates/reify-compiler/src/functions.rs:544`) under this cluster. **Ground-truth investigation re-homes it OUT of this PRD.** It is:
- **Not lost work.** The OpenVDB kernel adapter is real and on-disk (`crates/reify-kernel-openvdb/`: `ingest.rs::read_vdb_file` / `lower_to_sampled`, `kernel_real.rs`); `reify-eval/src/field_import_provenance.rs` carries the provenance builder. It is a **producer-orphan** (audit cluster C-17 / GR-003): `reify-kernel-openvdb` is not a dependency of `reify-eval`, the compiler front-end hard-errors `FieldImportedV02`, and `engine_eval.rs:620` returns a `Value::Undef` placeholder.
- **Already owned by a different PRD and tracked by live pending tasks.** `docs/prds/v0_3/multi-kernel-phase-3.md` §8 Phase 4 task θ owns the eval-side consumer arm (GR-003 contested-ownership disposition, 2026-05-12). Live tasks: **#3439** (eval arm θ, pending) and **#3576** (full front-end + eval + cache, pending — its `metadata.files` already includes `crates/reify-compiler/src/functions.rs`, `crates/reify-compiler/tests/field_compile_tests.rs`, and `crates/reify-types/src/diagnostics.rs`; its G2 signal is "compiles without `Severity::Error`" → "evaluates to `Value::Field` (not `Value::Undef`)" with probe-sample assertions).

Re-authoring it here would create a fourth instance of the known-contested `imported-field-source ↔ multi-kernel` seam (PRD overlay G4 list, item 2). The correct action is **declare the relationship and defer to the existing owner** — see §5 Cross-PRD relationship. This PRD's only obligation is one companion correction task (task ν) that links the imported-field PRDs to the now-existing `std.fields` module surface so the two stay coherent.

## §2 — Sketch of approach

Four independent stdlib-authoring slices, each a self-contained vertical (author `.ri` → register in loader → CI example imports it). No cross-slice dependency among the four except the shared loader-registration edit (additive, conflict-free — each appends a distinct tuple). One companion doc-correction task closes the §5 seam.

### Slice A — `std.ports` (mechanical / electrical / thermal / fluid)

Follow the `materials_*` flat-file convention: `ports_mechanical.ri` → module `std.ports.mechanical`, etc. (the spec tree's `mod.ri` nesting is aspirational; the loader uses flat dotted names — see `stdlib_loader.rs:55-130`).

- A root `ports.ri` → `std.ports` defining the base `Port` trait and a `Directionality` enum (`In`, `Out`, `Bidi`), matching the spec §7.6 prelude promise and the Rust-side `PortDirection` enum (`reify-types`, used by `connect.rs` compatibility checking). `Port` carries `param direction : Directionality`.
- `ports_mechanical.ri` → `std.ports.mechanical`: `MechanicalPort : Port`, then `Bore`, `Shaft`, `RotaryPort` (torque capacity, max speed), `ThreadedPort` (thread diameter, pitch), `StructurePort`.
- `ports_electrical.ri` → `std.ports.electrical`: `ElectricalPort : Port`, `PowerPort` (voltage, max current), `SignalPort` (signal type, impedance).
- `ports_thermal.ri` → `std.ports.thermal`: `ThermalPort : Port` (heat-transfer interface params).
- `ports_fluid.ri` → `std.ports.fluid`: `FluidPort : Port` (pressure, flow-rate, fluid medium).

Property dimensions use the existing 35 named dimension aliases (`Torque`, `AngularVelocity`, `Voltage`, `Current`, `Length`, `Pressure`, …). Where the spec reference asks for a dimension that does not exist (e.g. a flow-rate `Volume/Time`), express it as a type expression (`Volume / Time`) — valid per spec line 875 (`pub let torque_constant : Torque/Current`). Document any such deviation in a header comment per the `io.ri` precedent.

**Prelude re-export of `Port`/`Directionality` is OUT OF SCOPE for the authoring tasks** and split into a separate slice (task ε) — see §6 design decision 3. The authoring tasks require an explicit `import std.ports` in the consumer example; that is the observable signal.

### Slice B — `std.process` + DFMRule

Single file `process.ri` → `std.process`, per done-task #333's enumeration:
- `Process` (param `duration : Time`, param `cost : Money`) — the base trait.
- `Subtracting`, `Adding`, `Forming`, `Joining` (each `: Process`) — the four spec-named categories. Plus `Parting`, `SurfaceTreating`, `HeatTreating` from the original #333 description (in scope; they refine `Process` identically).
- `DFMRule` — the design-for-manufacturing rule trait (param `rule_name : String`, param `severity : String` or a `Severity` enum, and a process-applicability marker). DFM-rule *evaluation* (running a rule against a geometry) is NOT in scope — only the trait surface that lets a designer declare a rule. Document this boundary in the header.

### Slice C — `std.fields` module surface (packages existing builtins)

The differential operators are already built in (`gradient`/`divergence`/`curl`/`laplacian`/`sample` in `reify-expr`). `std.fields` is a **packaging-only** module — it does NOT reimplement them (G4 seam: declared consumer of the existing `calculus.rs` operators). It provides:
- The `Field<D, C>` type-alias surface (the parameterised field type — `Field<X,Y>` now parses in both param and return position, task 3088).
- `pub fn` wrappers / re-export documentation for the interpolation constructors named in the spec §11 tree (`constant_field`, `fn_field`, `from_samples`) and spatial ops (`compose`, `sample`, `restrict`) — authored as thin `pub fn` surfaces ONLY where a corresponding builtin or composable definition exists; where the operator is a prelude builtin (the differential ops), the module documents it as such rather than shadowing it (matching how `analysis.ri` does NOT redeclare `von_mises`).
- Follows the `analysis.ri` idiom: `#no_prelude`, `pub type` aliases, trait/structure surface, no redeclaration of intercepted builtins.

The decisive scoping question (which ops get a real `pub fn` body vs. a doc-only mention) is resolved at §6 design decision 4.

### Slice D — `std.units` physical constants

Append to `units.ri` (the existing `STANDARD_GRAVITY()` lives there), in the existing zero-arg `pub fn` idiom (Reify lacks top-level `const`):
- `SPEED_OF_LIGHT() -> Velocity { 299792458.0 * 1m / 1s }` — exact, SI definition (c = 299792458 m/s).
- `BOLTZMANN_CONSTANT() -> Energy / Temperature { 0.00000000000000000000001380649 * 1J / 1K }` — k_B = 1.380649e-23 J/K (2019 SI redefinition, exact). **Written in decimal form, NOT scientific notation** — `1.38e-23` does not parse in value position (verified §G3; convention documented at `modal_analysis.ri:32-33`).

The spec §11.3 also names `pi` and `g`; `pi` is already a compiler builtin (`constants.rs` `BUILTIN_NAMES`), `g` is the existing `STANDARD_GRAVITY()`. No change needed for those — note it in the task.

## §3 — Pre-conditions for activating

- **No upstream blockers.** All four slices use existing grammar (grammar_confirmed) and existing dimension aliases. The OpenVDB front-end (the only compiler-change item from the survey brief) is explicitly excluded and tracked elsewhere.
- Orchestrator stopped; batch stays `deferred` until a human flips it.

## §4 — Resolved design decisions

1. **One PRD, four slices, OpenVDB excluded.** The four stdlib slices share one mechanism (append `.ri` + register in `stdlib_loader.rs`), one substrate (existing grammar), one consumer surface (a designer's `import`). They cohere. The OpenVDB front-end is a compiler+eval change with a different blast radius and a live pending owner (#3576 / multi-kernel task θ); folding it in would duplicate tracked work and re-open a contested seam. Excluded with a declared seam (§5).

2. **Flat-file naming, not `mod.ri` nesting.** The loader maps flat files to dotted module names (`materials_mechanical.ri` → `std.materials.mechanical`). The spec §11.1 tree shows `ports/mod.ri` nesting, but no stdlib module on disk uses that shape. Follow the established convention: `ports.ri` → `std.ports`, `ports_mechanical.ri` → `std.ports.mechanical`. (Sequential growing-prelude compilation in `load_stdlib` requires `std.ports` to be registered before the `std.ports.*` submodules that refine `Port`.)

3. **`Port`/`Directionality` authored in `std.ports`; prelude re-export is a separate slice.** The spec §7.6 lists `Port` and `Directionality` as prelude contents, but they are absent today and `port x : T` does not enforce "T refines Port." Authoring the traits (slice A root file) is decoupled from wiring them into the implicit prelude (task ε), because prelude wiring touches `stdlib_loader.rs` / the prelude-seeding path and risks shadowing the Rust-side `PortDirection`. Authoring tasks require explicit `import std.ports`; the prelude-wiring task is what removes that requirement and is the observable signal that "T must refine Port" can eventually be enforced.

4. **`std.ports` does NOT enforce the "T refines Port" compile rule.** That is a compiler change (a new check in port-decl validation), out of scope for stdlib authoring. The named-port-type stdlib is the deliverable; enforcement is flagged as a design fork for Leo (could be a follow-on task once `Port` is in the prelude).

5. **`std.fields` packages, never reimplements.** The differential operators stay built-in; the module surface is documentation + `pub type Field<D,C>` + thin helpers only where a real composable body exists. No shadowing of intercepted builtins (the `analysis.ri` precedent).

6. **Constants use decimal literals + zero-arg `pub fn`.** Matches `STANDARD_GRAVITY()`. Scientific notation is unsupported in value position.

## §5 — Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_3/multi-kernel-phase-3.md` (§8 Phase 4 task θ) | this PRD references | OpenVDB `field def … imported` front-end re-enable: `functions.rs:544` `FieldImportedV02` error + `engine_eval.rs:620` `Value::Undef` arm → `reify-kernel-openvdb::ingest` | **multi-kernel-phase-3** (live tasks #3439, #3576) | tracked-elsewhere; NOT in this batch |
| `docs/prds/v0_2/imported-field-source.md` | this PRD's `std.fields` is the module home for the `imported` source kind once it lands | `field def … source = imported` lowers to a `Field<D,C>` value; `std.fields` documents the `Field` type that imported fields realize as | imported-field-source PRD (front-end) / this PRD (module surface) | companion correction (task ν) cross-links them |
| `docs/prds/v0_3/imported-field-source-hdf5-csv.md` | sibling of the above; HDF5/CSV follow-on | same `Field<D,C>` surface | imported-field PRDs | doc cross-link only |
| `reify-expr` (`calculus.rs`) — in-system, not a PRD | `std.fields` **consumes** | the built-in `gradient`/`divergence`/`curl`/`laplacian`/`sample` operators (`reify-expr/src/lib.rs:198-349`) | `reify-expr` (exists) | wired (builtins ship; module documents them) |
| `reify-types` (`PortDirection` enum) — in-system | `std.ports` **mirrors** | the Reify `Directionality` enum mirrors the Rust `PortDirection` (In/Out/Bidi) that `connect.rs` uses for port-compatibility checking | `reify-types` (exists) | wired (no reciprocal ambiguity; Reify-side is a new surface, Rust-side unchanged) |

No reciprocal-ownership ambiguity: the OpenVDB seam is unambiguously owned by multi-kernel-phase-3 (the 2026-05-12 GR-003 disposition already resolved it). This PRD only *defers* to that owner and adds one cross-link.

## §6 — Decomposition plan

Greek labels; actual IDs assigned at decompose time. Approach **bare B** (vertical slices, shallow DAG) — these are self-contained stdlib-authoring features, none touches a G5 load-bearing seam (no FEA / ComputeNode / persistent-naming / multi-kernel / parser change). Each leaf's observable signal is a CI `.ri` example that imports the module and exercises a trait/constant, compiling without `Severity::Error`.

- **Task α — Author `std.ports` root + mechanical submodule.**
  - Author `crates/reify-compiler/stdlib/ports.ri` (`std.ports`: `Port` trait + `Directionality` enum) and `ports_mechanical.ri` (`std.ports.mechanical`: `MechanicalPort`, `Bore`, `Shaft`, `RotaryPort`, `ThreadedPort`, `StructurePort`). Register both in `stdlib_loader.rs` (root before submodule). Header deviation block per `io.ri`.
  - **Observable signal:** a CI example `.ri` (`examples/stdlib/ports_mechanical.ri`) with `import std.ports.mechanical` declares `structure def Coupling<D: RotaryPort, N: RotaryPort> { port driver : D … }` and compiles with zero `Severity::Error` diagnostics (the stdlib loader's `assert!` on Error-severity is itself a hard gate — a broken module panics the whole build).
  - **Prereqs:** none.
  - **grammar_confirmed:** true.
  - **Modules touched:** reify-compiler (stdlib + stdlib_loader.rs), examples.

- **Task β — Author `std.ports` electrical / thermal / fluid submodules.**
  - `ports_electrical.ri` (`PowerPort`, `SignalPort`), `ports_thermal.ri` (`ThermalPort`), `ports_fluid.ri` (`FluidPort`). Register in `stdlib_loader.rs`.
  - **Observable signal:** `examples/stdlib/ports_domains.ri` imports `std.ports.electrical` + `.thermal` + `.fluid`, declares a structure with a `PowerPort` and a `FluidPort`, compiles clean.
  - **Prereqs:** α (refines the root `Port` trait; root must register first).
  - **grammar_confirmed:** true.
  - **Modules touched:** reify-compiler (stdlib + stdlib_loader.rs), examples.

- **Task γ — Author `std.process` + `DFMRule`.**
  - `process.ri` (`std.process`): `Process`, `Subtracting`, `Adding`, `Forming`, `Joining`, `Parting`, `SurfaceTreating`, `HeatTreating`, `DFMRule`. Register in loader. Header documents the trait-surface-only boundary (no rule evaluation).
  - **Observable signal:** `examples/stdlib/process.ri` imports `std.process`, declares a `structure def MilledPart : Subtracting { … duration = … cost = … }` and a `DFMRule`-conforming structure, compiles clean.
  - **Prereqs:** none.
  - **grammar_confirmed:** true.
  - **Modules touched:** reify-compiler (stdlib + stdlib_loader.rs), examples.

- **Task δ — Author `std.fields` module surface.**
  - `fields.ri` (`std.fields`): `pub type Field<D, C>` surface, doc-block cataloguing the prelude-builtin differential operators it packages (no shadowing), and `pub fn` helpers only for ops with a composable Reify body (resolve which during the task — see open Q1). Register in loader.
  - **Observable signal:** `examples/stdlib/fields.ri` imports `std.fields`, declares a `field def temp : Point3<Length> -> Temperature { … }`, calls `gradient(temp)` and `sample(temp, point3(…))`, and asserts a sampled value — compiles AND `reify eval` prints the expected gradient/sample value (the operators already work; the example proves the module-import path resolves them).
  - **Prereqs:** none (consumes existing builtins).
  - **grammar_confirmed:** true (verified `Field<X,Y>` parses param + return).
  - **Modules touched:** reify-compiler (stdlib + stdlib_loader.rs), examples.

- **Task ζ — Add `SPEED_OF_LIGHT` + `BOLTZMANN_CONSTANT` to `std.units`.**
  - Append two zero-arg `pub fn` constants to `units.ri` (decimal literals). Doc-comment cites the SI reference values.
  - **Observable signal:** `examples/stdlib/constants.ri` imports `std.units` (or relies on prelude), uses `SPEED_OF_LIGHT()` and `BOLTZMANN_CONSTANT()` in a dimensioned `let`, compiles clean and `reify eval` prints `c ≈ 2.998e8 m/s` and `k_B ≈ 1.38e-23 J/K` (values matched to the SI definitions: c = 299792458 m/s exactly; k_B = 1.380649e-23 J/K exactly).
  - **Prereqs:** none.
  - **grammar_confirmed:** true.
  - **Modules touched:** reify-compiler (stdlib/units.ri), examples.

- **Task ε — Wire `Port` + `Directionality` into the implicit prelude (spec §7.6).**
  - Make `std.ports`'s `Port` trait and `Directionality` enum re-exported through the implicit prelude, per spec §7.6 (which lists them as prelude contents). Touches the prelude-seeding path in `stdlib_loader.rs` / `module_dag.rs`.
  - **Observable signal:** a CI example `.ri` declares `port shaft : RotaryPort` WITHOUT an explicit `import std.ports` and compiles — i.e., `Port`/`Directionality` resolve from the prelude. Regression: existing examples still compile (no shadowing of the Rust `PortDirection`).
  - **Prereqs:** α (the `Port` trait must exist before it can be re-exported).
  - **grammar_confirmed:** true (no new syntax — prelude wiring is loader-side).
  - **Modules touched:** reify-compiler (stdlib_loader.rs / module_dag.rs), examples.

- **Task ν — Companion doc correction: cross-link imported-field PRDs to the `std.fields` surface.**
  - Update `docs/prds/v0_2/imported-field-source.md` and `docs/prds/v0_3/imported-field-source-hdf5-csv.md` to reference the now-existing `std.fields` module as the home of the `Field<D,C>` type that imported fields realize as. No code change; keeps the imported-field seam coherent with the reconstructed module. Cross-reference this PRD.
  - **Observable signal:** the two PRD docs updated with a `std.fields` cross-reference; doc lint passes; no code change.
  - **Prereqs:** δ (the `std.fields` surface must be authored before the PRDs can reference it).
  - **grammar_confirmed:** N/A (doc-only).
  - **Modules touched:** docs.

### Dependency view

```
α ──┬──→ β
    └──→ ε
γ  (independent)
δ ──→ ν
ζ  (independent)
```

## §7 — Out of scope

- **OpenVDB / any `imported` field-source front-end re-enable** — owned by `multi-kernel-phase-3.md` task θ; live tasks #3439, #3576. This PRD only cross-links (task ν).
- **Compile-time enforcement of "port type must refine `Port`"** — a new compiler check; flagged as a design fork. Not blocking the named-port-type stdlib.
- **DFM-rule evaluation** (running a `DFMRule` against geometry to emit violations) — only the trait surface is in scope.
- **HDF5 / CSV imported fields** — separate PRDs.
- **The `std.math`, `std.geometry`, `std.determinacy`, `std.tolerancing` module trees** from spec §11 — not part of this Bucket-3 lost-work set (either present or out of survey scope).
- **Reimplementing differential operators in `.ri`** — they are and stay built-in.

## §8 — Open questions (tactical; deferred to impl)

1. **`std.fields` helper surface breadth.** Which of `compose` / `sample` / `restrict` / `constant_field` / `fn_field` / `from_samples` get a real `pub fn` body vs. a doc-only "this is a prelude builtin" mention? **Suggested resolution:** grep `reify-expr` + `reify-compiler` for each name; author a `pub fn` only where there's no intercepting builtin and a composable Reify body is possible, doc-mention the rest. Decide during task δ.
2. **`DFMRule.severity` — `String` or an enum.** A `Severity { Info, Warning, Error }` enum reads better but adds a name that might collide with the compiler's diagnostic `Severity`. **Suggested resolution:** use a stdlib-local enum with a distinct name (e.g. `DFMSeverity`) to avoid confusion. Decide during task γ.
3. **`std.ports` thermal/fluid property set.** The spec names the domains but not the exact interface params for thermal/fluid ports. **Suggested resolution:** minimal defensible set (thermal: a heat-transfer-coefficient + temperature interface; fluid: pressure + flow-rate + medium), documented as a deviation. Decide during task β.
4. **Whether `Bore`/`Shaft` are port traits or geometry features.** Spec §11.3 lists them under `std.ports` mechanical. **Suggested resolution:** author them as `MechanicalPort`-refining traits (mating-interface ports), per the spec listing. Decide during task α.
