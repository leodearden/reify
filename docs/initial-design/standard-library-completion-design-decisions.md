# Standard Library Completion: Design Decisions

**Status:** Complete — covers `std.determinacy`, `std.fields`, `std.process`, `std.analysis`, `std.io`  
**Version:** 0.1 — First crystallisation from standard library completion design sessions  
**Builds on:** `standard-library-boundary-design-decisions.md` v0.1, `ontology-design-decisions.md` v0.1, `type-system-design-decisions.md` v0.1, `syntax-design-decisions.md` v0.1, `constraint-system-design-decisions.md` v0.1, `evaluation-graph-completion-design-decisions.md` v0.2

---

## 1. Design approach

This document completes the standard library specification by resolving the five remaining modules identified in `standard-library-boundary-design-decisions.md` §13.1. The modules were resolved in dependency order:

1. **`std.determinacy`** — predicates and purpose framework (consumed by everything else)
2. **`std.fields`** — field combinators and constructors (consumed by analysis)
3. **`std.process`** — manufacturing/transformation framework (depends on ports, materials, geometry, determinacy)
4. **`std.analysis`** — simulation integration (depends on fields, geometry, materials)
5. **`std.io`** — design boundary occurrences (depends on everything)

The central findings are:

- **Determinacy predicates are compiler intrinsics** — they inspect evaluation graph state that is not accessible to user-defined functions.
- **Fields are composed through typed combinators** — `Field<D, C>` is a compiler-intrinsic generic type with type-safe composition.
- **Process categories are traits, not enums** — a process can belong to multiple categories (e.g. brazing is both Joining and HeatTreating).
- **Analysis is parallel to Process, not subordinate** — analyses are occurrences but not physical transformations.
- **Design boundaries have four specialisations** — Source (Input, Buy) and Sink (Output, Discard) capture all boundary crossings.

---

## 2. Updates to prior documents

### 2.1 Currency as a dimension

`Currency` is added as a named dimension, making the dimension vector 9-dimensional (7 SI + Angle + Currency). Currency units are declared with the `unit` keyword like any other unit:

```
unit USD : Currency
unit GBP : Currency
unit EUR : Currency
unit JPY : Currency
```

All currency values within a project use a single uniform dimension with constant conversion factors. Time-varying exchange rates and accounting concerns are outside the design system's scope.

This enables natural expressions like `25USD/kg` for material cost per unit mass, and `Scalar<Currency>` as a parameter type for cost estimation throughout the process and procurement framework.

### 2.2 Module tree update

The module tree from `standard-library-boundary-design-decisions.md` §4 is updated:

```
std
├── ...                         — (unchanged modules from prior document)
│
├── process                     — manufacturing / transformation framework
│   ├── (mod.ri)                — Process trait
│   ├── categories              — Subtracting, Adding, Forming, Joining, Parting,
│   │                             SurfaceTreating, HeatTreating
│   └── dfm                     — DFMRule trait
│
├── io                          — design boundary occurrences
│   ├── (mod.ri)                — Source, Sink, Input, Buy, Output, Discard,
│   │                             Provenance, DisplayStyle
│   └── formats                 — STEPOutput, STLOutput, ThreeMFOutput,
│                                 DisplayOutput, STEPInput, PointCloudInput
│
├── analysis                    — simulation integration
│   ├── (mod.ri)                — Analysis trait
│   ├── stress                  — von_mises, principal_stresses, safety_factor, max_shear
│   └── result                  — AnalysisResult trait
│
├── fields                      — field manipulation utilities
│   ├── (mod.ri)                — Field<D, C> (compiler-intrinsic generic type)
│   ├── interpolation           — constant_field, fn_field, from_samples,
│   │                             InterpolationMethod
│   └── spatial                 — compose, sample, restrict, clamp_field, remap_field,
│                                 threshold, gradient, divergence, curl, laplacian
│
└── determinacy                 — determinacy predicates and purpose framework
    ├── (mod.ri)                — determined(), constrained(), undetermined(),
    │                             partially_determined(), AllParamsDetermined,
    │                             AllGeometryDetermined, RepresentationWithin
    └── purposes                — design_review, simulation_ready
```

`std.determinacy.defaults` is removed — robustness objectives are deferred to post-v0.1.

---

## 3. `std.determinacy`

### 3.1 `std.determinacy` (mod.ri)

Determinacy predicates are compiler intrinsics. They inspect the evaluation graph's determinacy state for a referenced parameter, which is not accessible to user-defined functions. All four predicates are in the prelude.

```
// Compiler intrinsics — in prelude

fn determined(param_ref) -> Bool           // has a specific value
fn constrained(param_ref) -> Bool          // has at least one constraint narrowing it
fn undetermined(param_ref) -> Bool         // is undef with no constraints
fn partially_determined(param_ref) -> Bool // constrained && !determined
```

Utility constraints for use inside purpose definitions:

```
constraint def AllParamsDetermined {
    param subject : Structure
    // compiler-intrinsic: walks all params of subject, asserts determined() on each
}

constraint def AllGeometryDetermined {
    param subject : Structure
    // compiler-intrinsic: walks all Geometry-typed params, asserts determined()
}

constraint def RepresentationWithin {
    param subject : Structure
    param tolerance : Length
    // asserts all geometry realisations are within tolerance of their ideal
}
```

`AllParamsDetermined` and `AllGeometryDetermined` are necessarily compiler intrinsics — they walk the structural graph, which user code cannot do generically over an arbitrary structure.

### 3.2 `std.determinacy.purposes`

Standard purposes demonstrating the pattern. These are deliberately minimal — domain libraries define the substantive purposes (`manufacturing_ready` needing process-specific checks, `thermal_analysis_ready` needing boundary conditions, etc.).

```
purpose design_review(subject: Structure) {
    constraint AllParamsDetermined { subject }
}

purpose simulation_ready(subject: Structure) {
    constraint AllGeometryDetermined { subject }
    constraint determined(subject.material)
        where subject : Physical
}
```

### 3.3 Design notes

**Robustness objectives deferred.** The concept of preferring solutions far from constraint boundaries is sound, but the mechanism (expressing "distance from constraint boundary" generically) depends on constraint solver internals not yet specified. Deferred to post-v0.1.

**Purpose is syntactic sugar.** As established in `evaluation-graph-completion-design-decisions.md` §5.1, activating a purpose applies its constraints; deactivating removes them. No special scheduling machinery. The checking/solving/proposing mode falls out of input determinacy state.

---

## 4. `std.fields`

### 4.1 `std.fields` (mod.ri)

`Field<D, C>` is a compiler-intrinsic generic type, where `D` is the domain type and `C` is the codomain type. Fields are opaque mathematical objects at the language level (like geometry). The compiler/runtime manages their representation; user code composes them through typed combinators.

```
// Compiler-intrinsic generic type
// Field<D, C> where D is domain type, C is codomain type
//
// Examples:
//   Field<Point3<Length>, Scalar<Temperature>>   — temperature distribution
//   Field<Point3<Length>, Vector3<Force>>         — force field
//   Field<Real, Scalar<Length>>                   — 1D profile (cam, spring rate)
//   Field<Point3<Length>, Tensor<2, 3, Pressure>> — stress tensor field
```

The two type parameters make field composition type-safe: composing a `Field<Point3<Length>, Scalar<Temperature>>` with a `Field<Scalar<Temperature>, Scalar<Pressure>>` yields a `Field<Point3<Length>, Scalar<Pressure>>`.

### 4.2 `std.fields.interpolation`

Constructors for building fields from discrete data and analytical definitions.

```
enum InterpolationMethod { Linear, Bilinear, Trilinear, NearestNeighbour, RBF, Kriging }

fn constant_field<D, C>(value: C) -> Field<D, C>

fn fn_field<D, C>(f: fn(D) -> C) -> Field<D, C>

fn from_samples<D, C>(
    points: List<D>,
    values: List<C>,
    method: InterpolationMethod = Linear
) -> Field<D, C>
```

`from_samples` is the single entry point for all discrete-data interpolation. The `InterpolationMethod` enum selects the algorithm. This keeps the API surface small while supporting multiple interpolation strategies.

### 4.3 `std.fields.spatial`

Combinators for composing, transforming, and querying fields.

```
// Composition — codomain of f must match domain of g
fn compose<A, B, C>(f: Field<A, B>, g: Field<B, C>) -> Field<A, C>

// Evaluation
fn sample<D, C>(field: Field<D, C>, at: D) -> C

// Domain restriction
fn restrict<D, C, G>(field: Field<D, C>, region: G) -> Field<D, C>
    where G: Geometry

// Codomain manipulation
fn clamp_field<D, Q: Dimension>(
    field: Field<D, Scalar<Q>>, lo: Scalar<Q>, hi: Scalar<Q>
) -> Field<D, Scalar<Q>>

fn remap_field<D, Q: Dimension>(
    field: Field<D, Scalar<Q>>,
    from: Range<Scalar<Q>>,
    to: Range<Scalar<Q>>
) -> Field<D, Scalar<Q>>

fn threshold<D, Q: Dimension>(
    field: Field<D, Scalar<Q>>, value: Scalar<Q>
) -> Field<D, Bool>

// Differential operators — @optimised, implementation may be partial
fn gradient<N: Nat, Q: Dimension>(
    field: Field<Point<N, Length>, Scalar<Q>>
) -> Field<Point<N, Length>, Vector<N, Q / Length>>

fn divergence<N: Nat, Q: Dimension>(
    field: Field<Point<N, Length>, Vector<N, Q>>
) -> Field<Point<N, Length>, Scalar<Q / Length>>

fn curl<Q: Dimension>(
    field: Field<Point3<Length>, Vector3<Q>>
) -> Field<Point3<Length>, Vector3<Q / Length>>

fn laplacian<N: Nat, Q: Dimension>(
    field: Field<Point<N, Length>, Scalar<Q>>
) -> Field<Point<N, Length>, Scalar<Q / Length^2>>
```

### 4.4 Design notes

**Differential operator return types.** `gradient` returns `Q / Length` — the dimension of the spatial derivative. For a temperature field, the gradient has dimension `Temperature / Length`. `curl` is 3D-only. `laplacian` has dimension `Q / Length^2`. These are dimensionally exact.

**`@optimised` differential operators.** All differential operators have well-defined language-level semantics but practical implementation depends on the field representation. Implementations may be partial in early versions — not all field representations will support all operators.

**`restrict` takes `Geometry`.** A field can be restricted to a volume, surface, or curve region, matching the general `Geometry` supertrait pattern used elsewhere.

---

## 5. `std.process`

### 5.1 `std.process` (mod.ri)

A Process is a trait on occurrences. It models a physical operation/event/process that has some design-relevant effect — creating, destroying, transforming, constraining, or allocating spatial entities within the sequential scope of the design.

Occurrences have ports (typed interaction points), through which they receive and emit spatial entities (structures and fields) from/to other occurrences. The process graph is formed by connections between occurrence ports. Process compatibility is validated by the type system through port typing — no separate chain-validation mechanism is needed.

```
trait Process {
    param duration : Time = undef
    param cost : Scalar<Currency> = undef
}
```

`Process` is deliberately thin. Inputs and outputs are expressed structurally via occurrence ports, not as explicit parameters on the trait. This composes cleanly with the occurrence model established in the ontology.

**Process vs other occurrences.** Not all occurrences are processes. Analysis (§6) and boundary occurrences (§7) are occurrences but not processes. Process specifically models physical transformation.

**Assembly.** `Assemble` is an occurrence (verb form) implementing the `Process` trait. An `Assembly` is a structure (noun form). Occurrence defs and structure defs live in the same namespace, so distinct names are required. By convention, occurrence defs use verb forms and structure defs use noun forms.

### 5.2 `std.process.categories`

Process category traits use uniform gerund (-ing) grammatical form. A process can implement multiple category traits (e.g. brazing is both `Joining` and `HeatTreating`).

```
trait Subtracting : Process {
    param tool_access : Geometry = undef
    param min_feature_size : Length = undef
    param achievable_finish : Length = undef    // Ra
}

trait Adding : Process {
    param layer_thickness : Length = undef
    param min_feature_size : Length = undef
    param build_volume : Solid = undef
}

trait Forming : Process {
    param min_bend_radius : Length = undef
    param max_draw_depth : Length = undef
    param draft_angle : Angle = undef
}

trait Joining : Process {
    param joint_strength : Pressure = undef
    param reversible : Bool = undef
}

trait Parting : Process {
    param kerf_width : Length = undef
    param min_feature_size : Length = undef
}

trait SurfaceTreating : Process {
    param coating_thickness : Length = undef
    param achievable_finish : Length = undef    // Ra
}

trait HeatTreating : Process {
    param treatment_temperature : Temperature = undef
    param hold_duration : Time = undef
}
```

### 5.3 `std.process.dfm`

The DFM rule trait is the coordination point for design-for-manufacturing constraints. It relates a structural feature to a process capability. Concrete rules (`MinWallThickness`, `MaxAspectRatio`, `DraftAngle`, etc.) are defined in domain libraries.

```
trait DFMRule {
    param subject : Structure
    param process : Process
}
```

### 5.4 Design notes

**Gerund form rationale.** The previous draft mixed adjectives (`Subtractive`, `Additive`), gerunds (`Forming`, `Joining`), and nouns (`HeatTreatment`). Gerunds were chosen for uniformity: they read naturally as traits on occurrences ("this occurrence *is subtracting*"), they're uniformly derivable from verb roots, and they parallel the convention that occurrence defs are verb-like.

**Parting as dual of Joining.** Parting covers cutting, slitting, shearing, sawing — operations that separate a structure into multiple structures. This is the process that produces offcuts handled by `Discard` (§7).

**SurfaceTreating subsumes Coating.** Plating, etching, painting, polishing, flame polishing, carburising, nitriding, and other surface modifications are all surface treatments. `Coating` was too narrow.

**Category trait parameters are abstract.** Each category carries only the parameters that DFM constraint authors universally need to reference. Specific process parameters (laser power for SLM, spindle speed for milling, current density for electroplating) belong in domain libraries extending these traits.

**Process compatibility via type system.** If a milling occurrence has an output port typed `Solid` and a plating occurrence has an input port requiring `Solid + SurfaceTreatable`, the type checker validates the connection. Ordering constraints (can't heat-treat before forming) are captured by port typing — a heat treatment input port requires structural properties that only a formed part would have. No separate chain-validation constraint is needed.

---

## 6. `std.analysis`

### 6.1 `std.analysis` (mod.ri)

An analysis is an occurrence that evaluates properties of a structure without physically transforming it. `Analysis` is a trait on occurrences, parallel to `Process` — not subordinate to it.

```
trait Analysis {
    param mesh_resolution : Length = undef
    param convergence_target : Real = undef
}
```

**Long-term integration.** The v0.1 model supports both external solver result import (via `AnalysisResult`, §6.3) and native solver integration. The target architecture is solvers as full evaluation graph nodes — progressive, warm-startable, committable — producing fields directly as occurrence outputs. As solvers are plumbed in, `AnalysisResult` becomes less central.

### 6.2 `std.analysis.stress`

Post-processing functions on stress tensor fields. These transform `Field<Point3<Length>, Tensor<2, 3, Pressure>>` into scalar fields or scalar values.

```
fn von_mises(
    stress: Field<Point3<Length>, Tensor<2, 3, Pressure>>
) -> Field<Point3<Length>, Scalar<Pressure>>

fn principal_stresses(
    stress: Field<Point3<Length>, Tensor<2, 3, Pressure>>
) -> List<Field<Point3<Length>, Scalar<Pressure>>>

fn safety_factor(
    stress: Field<Point3<Length>, Tensor<2, 3, Pressure>>,
    yield_strength: Pressure
) -> Field<Point3<Length>, Scalar<Dimensionless>>

fn max_shear(
    stress: Field<Point3<Length>, Tensor<2, 3, Pressure>>
) -> Field<Point3<Length>, Scalar<Pressure>>
```

### 6.3 `std.analysis.result`

Coordination trait for importing external simulation results into the design system. This is a stopgap for v0.1 — once solvers are native evaluation graph nodes, analysis occurrences produce fields directly.

```
trait AnalysisResult {
    param source : String
    param mesh : Geometry = undef
}
```

Concrete result types (stress results, thermal results, modal results) are defined in solver-specific domain libraries, not in `std`. The trait provides the coordination point.

### 6.4 Design notes

**Stress-only post-processing in v0.1.** Stress analysis is the most universal structural analysis output. Thermal, modal, and CFD post-processing are incremental additions for future versions.

**`safety_factor` takes `yield_strength` as a parameter,** not from a material trait, because the relevant strength depends on the failure mode being assessed (yield vs ultimate vs fatigue limit). The caller selects the appropriate strength.

---

## 7. `std.io`

### 7.1 `std.io` (mod.ri)

Design boundary occurrences are organised into two abstract traits — **Source** (something enters the design scope) and **Sink** (something leaves the design scope) — each with two specialisations.

```
// Abstract boundary traits
trait Source {}
trait Sink {}
```

#### Source specialisations

```
trait Input : Source {
    param source : String
    param provenance : Provenance = undef
}

trait Buy : Source {
    param supplier : String = undef
    param part_number : String = undef
    param unit_cost : Scalar<Currency> = undef
    param lead_time : Time = undef
}
```

**`Input`** brings data/geometry into the design from external tools — STEP files, point clouds, simulation results.

**`Buy`** introduces a physical item as a purchased/supplied part. Off-the-shelf parts (bolts, O-rings, stepper motors, ballscrews) enter the process graph through `Buy` occurrences. The part's *structure* is defined in a catalogue library; `Buy` is the occurrence that introduces it into the process graph with procurement metadata.

#### Sink specialisations

```
trait Output : Sink {
    param format : OutputFormat = undef
}

trait Discard : Sink {
    param reason : DiscardReason = undef
    param disposal_method : DisposalMethod = undef
}

enum DiscardReason { Offcut, Scrap, FailedInspection, Waste }
enum DisposalMethod { Recycle, Landfill, Reprocess }
```

**`Output`** emits data/geometry to external tools or displays — STEP export, STL export, viewport rendering.

**`Discard`** models material/parts leaving the design as waste. A parting operation produces the desired part and an offcut; the offcut is discarded. Failed inspection parts are discarded. This is important for cost estimation (recycled offcuts have recovery value) and environmental analysis.

#### Supporting types

```
structure def Provenance {
    param source_tool : String = undef
    param source_version : String = undef
    param timestamp : String = undef          // ISO 8601; no Date type in v0.1
    param tolerance_guarantee : Length = undef
}

enum OutputFormat { STEP, STL, ThreeMF, Display }
```

`Provenance` captures where imported data came from and its reliability. This feeds the tolerance contract system when working with external geometry.

### 7.2 `std.io.formats`

Format-specific Output and Input occurrence definitions for universally needed formats. Niche formats (IGES, Parasolid, AMF, proprietary) belong in domain libraries.

#### Output formats

```
occurrence def STEPOutput : Output {
    param subject : Structure
    param version : STEPVersion = AP214
    constraint determined(subject.geometry)
        where subject : Physical
}

enum STEPVersion { AP203, AP214, AP242 }

occurrence def STLOutput : Output {
    param subject : Solid
    param resolution : Length
}

occurrence def ThreeMFOutput : Output {
    param subject : Structure
    param include_materials : Bool = true
    param include_colors : Bool = true
}

occurrence def DisplayOutput : Output {
    param subject : Geometry
    param pane : Int = 0
    param style : DisplayStyle = undef
}

structure def DisplayStyle {
    param color : Vector3<Dimensionless> = undef    // RGB 0-1
    param opacity : Real = 1.0
    param wireframe : Bool = false
}
```

#### Input formats

```
occurrence def STEPInput : Input {
    param result : Structure
    param version : STEPVersion = undef
}

occurrence def PointCloudInput : Input {
    param result : PointCloud
    param format : PointCloudFormat = undef
}

enum PointCloudFormat { PLY, PCD, XYZ, LAS }
```

### 7.3 Design notes

**Source/Sink as the general boundary abstraction.** Every design boundary crossing is either a Source (something enters) or a Sink (something leaves). Input/Output handle data boundaries; Buy/Discard handle physical boundaries. This cleanly captures the full set of boundary crossings in a mechatronic design.

**Input and Output are not Processes.** They are boundary occurrences — they sit at the edge of the design scope. A `Buy` occurrence does not model the supplier's manufacturing process; it models the act of procurement from the design's perspective.

**`Buy` enables process graph completeness.** A bolted joint's process chain starts with `Buy` occurrences for the bolt and nut, followed by an `Assemble` occurrence. Without `Buy`, off-the-shelf parts appear from nowhere in the process graph.

**`Discard` enables material accounting.** A `Parting` operation produces two structures: the desired part and the offcut. The offcut feeds a `Discard` sink. This closes the material balance and enables cost estimation (scrap cost, recycling recovery).

**`DisplayOutput` for viewport rendering.** This is the mechanism for "display this geometry in pane #2." The `pane` parameter identifies the target viewport. `DisplayStyle` provides minimal rendering control. Full visualisation configuration (lighting, camera, sections) is runtime/IDE infrastructure outside the language.

---

## 8. Summary of key decisions

| Decision | Choice | Rationale |
|---|---|---|
| Determinacy predicates | Compiler intrinsics (`determined`, `constrained`, `undetermined`, `partially_determined`) | Inspect evaluation graph state inaccessible to user-defined functions |
| Purpose utilities in `std` | `AllParamsDetermined`, `AllGeometryDetermined`, `RepresentationWithin` + two example purposes | Building blocks for domain-specific purposes; examples demonstrate the pattern |
| Robustness objectives | Deferred to post-v0.1 | Mechanism depends on constraint solver internals not yet specified |
| `Field<D, C>` | Compiler-intrinsic generic type with domain and codomain type parameters | Type-safe field composition; matches opaque-object pattern used for geometry |
| Field interpolation | Single `from_samples` entry point with `InterpolationMethod` enum | Small API surface; method selection without proliferating constructors |
| Differential operators | Included in v0.1 (`gradient`, `divergence`, `curl`, `laplacian`); `@optimised`, implementation may be partial | Signatures are well-defined; partial implementation acceptable in early versions |
| Currency dimension | 9th dimension in exponent vector; constant conversion factors within project scope | Enables `25USD/kg`; accounting concerns outside design system |
| `Process` trait | Thin — `duration` and `cost` only; inputs/outputs via occurrence ports | Composes with occurrence model; avoids duplicating port semantics |
| Process categories | Gerund form traits: Subtracting, Adding, Forming, Joining, Parting, SurfaceTreating, HeatTreating | Uniform grammatical form; traits allow multiple categories per process |
| Parting | Included as dual of Joining | Covers cutting, slitting, shearing; produces offcuts for Discard |
| SurfaceTreating | Replaces Coating | Subsumes plating, etching, painting, polishing, carburising, nitriding |
| Process compatibility | Validated by type system via port typing | No separate chain-validation mechanism needed |
| DFM framework | `DFMRule` trait relating subject and process; concrete rules in domain libraries | Coordination point in `std`; domain-specific rules in libraries |
| Analysis | Parallel to Process, not subordinate | Analyses evaluate properties without physical transformation |
| Stress post-processing | `von_mises`, `principal_stresses`, `safety_factor`, `max_shear` | Most universal structural analysis output; other domains added incrementally |
| `AnalysisResult` | Thin trait; concrete result types in domain libraries | Stopgap for v0.1; target is native solver integration as evaluation graph nodes |
| Boundary abstraction | Source/Sink with four specialisations: Input, Buy, Output, Discard | Captures all design boundary crossings — data and physical |
| `Buy` | Source trait with procurement metadata | Off-the-shelf parts enter process graph; enables cost estimation |
| `Discard` | Sink trait with reason and disposal method | Closes material balance; enables cost and environmental analysis |
| `DisplayOutput` | Output occurrence with pane and style | Render-to-viewport as a first-class design boundary |
| Format scope in `std` | STEP, STL, 3MF, Display (output); STEP, PointCloud (input) | Near-universal formats as coordination points; niche formats in libraries |

---

*Document generated from standard library completion design sessions. Intended to be read alongside `standard-library-boundary-design-decisions.md` v0.1, which specifies `std.math`, `std.units`, `std.geometry`, `std.structural`, `std.ports`, `std.materials`, and `std.tolerancing`.*
