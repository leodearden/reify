# Iconic CAD + Reify: Integration Analysis

**Date:** 2026-04-02
**Status:** Research note

---

## 1. Iconic CAD vs Reify: Compare and Contrast

### 1.1 Shared Vision

Both projects reject the status quo of proprietary, GUI-centric CAD tools and aim to democratize engineering design. Both want non-traditional users (non-engineers in Iconic's case, LLMs in Reify's) to produce production-grade, manufacturable output. Both are open-source (Reify is AGPL-3.0, Iconic CAD follows OSE's open-source philosophy). Both emphasize parametric design with reusable modules.

### 1.2 Fundamental Divergence: Text vs Icons

| | **Reify** | **Iconic CAD** |
|---|---|---|
| **Input** | Text DSL (`.ri` files) | Drag-and-drop SVG icons in a visual editor |
| **Primary author** | Human engineer + LLM co-authoring | Non-engineer citizen designer |
| **Formalism** | Full programming language (types, constraints, traits, modules, solver) | Visual layout -> AI interprets -> generates CAD |
| **AI role** | Co-author of source code; syntax designed for reliable LLM generation | Post-hoc interpreter that converts icon layouts into CAD models |

Reify treats AI as a writing partner -- the DSL syntax is deliberately designed for LLM parseability (LL(k), regular structure, keywords over symbols). Iconic CAD treats AI as a translator -- it bridges a visual icon language to parametric CAD after the fact.

### 1.3 Technical Depth

| | **Reify** | **Iconic CAD** |
|---|---|---|
| **Constraint system** | First-class: `constraint`, `auto`, argmin solver, determinacy checking | Encoded implicitly in pre-built modules |
| **Type system** | Physical quantities with dimensional analysis (`5mm`, `3.2kN`), traits, enums, generics | Not applicable -- operates at module/assembly level |
| **Geometry kernel** | OpenCASCADE (OCCT) via `reify-kernel-occt` | FreeCAD (which also uses OCCT internally) |
| **Validation** | Compiler-enforced: type checking, constraint satisfaction, determinacy analysis | 4-stage pipeline: syntactic -> semantic -> stress test -> edge case |
| **Output** | Evaluated geometry, STEP export, eventually CAM/3MF | CAD models, BOMs, technical drawings, build procedures, cost estimates |
| **Stack** | Rust, Tree-sitter, Tauri GUI, LSP, MCP | FreeCAD + AI + SVG icons + wiki documentation |

Both ultimately sit on OCCT for geometry, but Reify owns the entire pipeline from lexer to kernel, while Iconic CAD delegates to FreeCAD.

### 1.4 Scope and Ambition

**Reify** is a language and compiler -- a new foundational tool for expressing engineering intent. It is general-purpose within mechanical/mechatronic design. Think "Rust for CAD" -- a clean-slate language that bridges abstract specification to manufacturing-ready output.

**Iconic CAD** is a workflow/protocol -- not a new tool, but a new way of using existing tools (FreeCAD, AI, graphic editors). It is specifically scoped to OSE's mission: open-source housing and the 50 GVCS machines. Think "Canva for infrastructure design."

### 1.5 Maturity

| | **Reify** | **Iconic CAD** |
|---|---|---|
| **Codebase** | ~15 Rust crates, 608 tasks completed, 1200+ tests, LSP, GUI, MCP server | Wiki specification pages, no public codebase |
| **Stage** | Late alpha -- compiler, solver, geometry, LSP, GUI all functional | Early concept/prototype -- being tested at Factor e Farm |
| **Runnable?** | Yes (`reify check`, `reify build`, `reify eval`) | No standalone tool -- it is a documented process |

Reify is substantially more mature as software. Iconic CAD is more mature as a design philosophy applied to real-world builds (OSE has built actual houses with their module system).

### 1.6 Complementarity

These projects are not competitors -- they are complementary:

- Iconic CAD's icon-to-CAD pipeline needs a formal language to express the parametric modules behind each icon. Reify's constraint-aware, LLM-friendly DSL could be that language.
- Reify needs a library ecosystem of reusable engineering modules. OSE's part libraries (20 trades x 50 tools/materials) are exactly that kind of content.
- Iconic CAD's "AI parses icons -> generates CAD" step is currently underspecified. Reify's deterministic compiler with `auto` resolution could make that step rigorous rather than probabilistic.

Iconic CAD is a UX vision for who designs and how. Reify is a computational foundation for what the design means and how it is validated. One is top-down from user experience, the other is bottom-up from formal semantics.

---

## 2. Suitability of Reify as Infrastructure for Iconic CAD

### 2.1 Direct Mappings

Iconic CAD's weakest link is the hand-wavy "AI parses the drawing and produces real CAD" step. Reify replaces that with a rigorous pipeline:

| Iconic CAD need | Reify capability |
|---|---|
| "Icons linked to parametric CAD" | `structure def` with `param`, `constraint`, `meta` blocks |
| "Expertise-embedded modules" | Trait system (`Rigid`, `Elastic`, `Joining`) encodes engineering rules as constraints |
| "Drag-and-drop assembly" | `sub` instantiation + `connect`/`chain` wiring -- the icon layout *is* the assembly topology |
| "BOM generation" | `Buy` pattern with `supplier`, `part_number`, `unit_cost` params -- cost rolls up through the tree |
| "Technically correct by construction" | Constraint solver + determinacy checking -- designs cannot be incomplete without the compiler telling you |
| "20 trades x 50 tools" | Module system with `pub` re-exports -- `std.mechanical.fasteners`, `std.electrical.wiring`, etc. |
| "100x compression" | `auto` resolution -- specify constraints, let the solver fill in parameters |

### 2.2 What Reify Provides That "FreeCAD + AI" Cannot

- **Determinacy guarantees**: Reify's compiler knows whether a design is fully determined, underdetermined, or overconstrained. Iconic CAD's AI step has no such guarantee -- it might hallucinate parameters.
- **Constraint propagation**: Change one icon's parameter and Reify re-solves the entire assembly. FreeCAD's parametric engine is per-part, not cross-assembly.
- **Formal interfaces**: Trait-typed ports mean an icon physically cannot connect to an incompatible icon. The type system prevents nonsensical assemblies at compile time.
- **LLM-friendly syntax**: If AI is part of the pipeline, Reify's LL(k) grammar was explicitly designed for reliable LLM generation. FreeCAD's Python API was not.

### 2.3 Gaps to Close

The gap between Reify's current state and Iconic CAD's needs is narrow:

1. **UI metadata conventions in `meta` blocks** -- standardize `icon_uri`, `category`, `display_name`, `searchable_tags`. The `meta` block exists today but only supports string values and has no standard field names. Minor language extension.

2. **Module-level `meta`** -- currently `meta` attaches to entities, not modules. Iconic CAD needs library-level metadata (trade category, icon set, description). Small compiler addition.

3. **Catalog browsing in tooling** -- enumerate all `pub structure def`s in a module tree, expose their `meta` and parameter signatures. This is tooling/GUI work, not language work.

4. **`std.process` implementation** -- the trait hierarchy (`Subtracting`, `Adding`, `Forming`, `Joining`) is designed in the spec but not yet implemented in the stdlib. This is already in the task queue (task #333: "Stdlib: Process traits + DFMRule") along with 30 other stdlib tasks covering geometry primitives, material traits, tolerancing, IO traits, and analysis functions.

---

## 3. Icon View Editor Integration into the Reify GUI

### 3.1 Current GUI Architecture

The Reify GUI is a Tauri desktop app with a SolidJS frontend:

- **Panel-based layout**: CSS Grid with resizable splitters -- Editor, 3D Viewport, PropertyEditor, ConstraintPanel, ChatPanel
- **Reactive stores**: `EngineStore` (meshes, values, constraints), `EditorStore` (open files), `SelectionStore` (selected/hovered entity) -- decoupled from any specific view
- **Event-driven backend**: Tauri IPC commands (`update_source`, `set_parameter`, `export`, etc.) emit typed events that stores subscribe to
- **3D viewport**: Three.js + BVH raycasting, isolated in its own module
- **No existing 2D canvas or SVG infrastructure**

### 3.2 What Makes Integration Tractable

- **Modular panel system**: Each panel is an independent SolidJS component. Adding `IconPanel` follows the same pattern as every existing panel.
- **Shared stores**: An icon editor would read/write the same `EngineStore` and `SelectionStore` the 3D viewport uses. Clicking an icon selects it in 3D; clicking a mesh highlights the corresponding icon. Free synchronization.
- **Extensible backend**: Adding new Tauri commands (`instantiate_from_icon`, `connect_ports`) is a function in `commands.rs` + a type in `bridge.ts`.
- **Tree-sitter parser**: The `.ri` AST is already available for bidirectional mapping between source text and visual elements.

### 3.3 Components to Build

| Component | Est. tasks | Notes |
|---|---|---|
| **2D canvas/SVG panel** | ~15-20 | Needs a library (Konva, Fabric.js, or raw SVG with SolidJS reactivity). Icons as draggable SVG elements on a canvas with grid snapping. |
| **Icon palette/browser** | ~15-20 | Enumerate `pub structure def`s from loaded modules, render their `meta.icon_uri` SVGs, support drag-from-palette-to-canvas. |
| **Layout -> Reify source generation** | ~20-30 | Icon positions + connections on canvas emit valid `.ri` source (`sub` declarations + `connect` statements). Essentially a visual-to-text compiler. |
| **Bidirectional sync** | ~20-30 | Edits in the text editor update the icon layout, and vice versa. Requires mapping between AST nodes and canvas elements. The LSP and Tree-sitter parser provide the AST; a layout algorithm handles the reverse direction. |
| **Port visualization** | ~8-10 | Show `in`/`out` ports on icons as connection points. Drag-to-connect UX that generates `connect` statements. |
| **Constraint feedback overlay** | ~8-10 | Show violated constraints as red indicators on the affected icons. Already available from `ConstraintPanel` data. |

**Total: ~85-120 tasks.** See section 3.5 for time estimates based on observed throughput.

### 3.4 The Bidirectional Sync Problem

This is the hardest part. Two approaches:

**Approach A -- Source-of-truth in `.ri` text (recommended):** The icon editor is a *view* of the AST. Edits in the icon editor emit AST mutations that get serialized back to text. Text edits trigger re-parse and icon layout update. This keeps Reify's compiler as the single source of truth and preserves LLM co-authoring.

**Approach B -- Source-of-truth in icon layout:** The icon canvas is the primary artifact, and `.ri` source is generated from it. Simpler to build but loses the power of text editing and LLM co-authoring.

Approach A is harder but aligns with Reify's "text is the canonical representation" philosophy.

### 3.5 Implementation Pace and Time Estimates

Reify's development has been heavily accelerated by an AI-driven "dark factory" orchestrator -- autonomous agents working tasks in parallel from a task queue, with human oversight for escalations and code review. The observed throughput:

**Ramp-up phase (Mar 13-24):** The project went from initial commit to a working compiler, solver, geometry kernel, and GUI in 12 days. Task tracking during this period was via branch merges rather than formal status transitions. Merge rate:

| Period | Task merges/day |
|---|---|
| Mar 13-16 | ~5 (bootstrapping, manual) |
| Mar 17-19 | 15-20 (orchestrator coming online) |
| Mar 20-24 | 20-65 (orchestrator at scale, with ramp-up) |

**Sustained phase (Mar 25 - Apr 2):** With formal task status tracking and the orchestrator running at capacity:

| Date | Tasks completed |
|---|---|
| Mar 25 | 5 (orchestrator restart) |
| Mar 26 | 26 |
| Mar 27 | 45 |
| Mar 28 | 124 (peak day) |
| Mar 29 | 32 |
| Mar 30 | 16 (low activity) |
| Mar 31 | 43 |
| Apr 2 | 138 (current peak) |

**Sustained average: ~50 tasks/day.** Peak days hit 120-140. Low days (weekends, orchestrator restarts, escalation handling) drop to 15-30.

**Current status:** 608 of 822 tasks complete (74%). 205 tasks remain (194 pending, 11 in-progress). Of those, 31 are stdlib tasks that directly support Iconic CAD integration (process traits, material traits, tolerancing, IO/Buy patterns, units).

**Time estimates for Iconic CAD integration work:**

| Work package | Tasks | At 50/day | At 30/day (conservative) |
|---|---|---|---|
| Remaining stdlib (already queued) | ~31 | < 1 day | ~1 day |
| Remaining v0.1 tasks (already queued) | ~205 | ~4 days | ~7 days |
| Icon editor (new work) | ~85-120 | 2-3 days | 3-4 days |
| First usable icon-to-3D demo | ~30-40 | < 1 day | 1-2 days |

These estimates assume the orchestrator is running at observed capacity. The icon editor work would need to be specified in a PRD and decomposed into tasks before it enters the queue, which adds ~1 day of planning overhead.

A first usable demo (drag icons from palette -> generates `.ri` source -> see 3D geometry) could be operational within a few days of starting the work. Full bidirectional sync with port visualization and constraint overlays would follow within a week.

---

## 4. Value of Reify's Process Modeling for OSE

### 4.1 What Reify's Occurrence System Can Model Today

Reify's `occurrence` entity type models manufacturing/assembly transformations with typed `in`/`out` ports, process parameters, and constraints:

```
occurrence def SawCut {
    port stock : in WorkPort { param material : Material }
    port piece : out WorkPort { param length : Length }
    param blade_type : String
    param cut_angle : Angle = 90deg
    constraint piece.length > 0mm
}

occurrence def DrillHole {
    port workpiece : in WorkPort
    port drilled : out WorkPort
    param bit_diameter : Length
    param depth : Length
    constraint bit_diameter > 0mm
    constraint depth <= workpiece.thickness
}

structure def WallFrameProcess {
    sub cut_studs = SawCut { blade_type = "crosscut" }
    sub cut_plates = SawCut { blade_type = "crosscut" }
    sub drill_plates = DrillHole { bit_diameter = 4mm }
    sub assemble = NailGun { nail_length = 89mm }

    chain cut_studs -> assemble
    chain cut_plates -> drill_plates -> assemble

    constraint cut_studs.piece.length == wall_height - 2 * plate_thickness
}
```

This gives OSE something they do not have today: a machine-checkable specification that validates process parameter consistency across steps. If someone changes the wall height, the constraint solver automatically updates everything that depends on it (and flags anything that is now invalid).

### 4.2 High-Value Capabilities for OSE

- **`purpose manufacturing_ready`** -- a formal predicate that checks "are all parameters determined for fabrication?" This directly maps to Iconic CAD's goal of "technically correct by construction."

- **Port trait contracts** -- if a `DrillHole` occurrence requires `workpiece.thickness > bit_diameter`, that constraint propagates through the chain. You physically cannot specify an impossible process.

- **`Buy` occurrences** -- `Buy(bolt, supplier="McMaster", part_number="91251A190", unit_cost=0.12USD)` makes BOM generation automatic and auditable.

- **Constraint-based tolerancing** -- tolerance flows through the chain: machining output tolerance becomes heat treatment input tolerance. No manual tolerance stack-up.

- **Determinacy checking** -- the compiler can answer "is this build procedure complete?" formally, across every parameter in every step.

### 4.3 Gaps for Rigorous Process Specs

| Gap | Impact | Difficulty to add |
|---|---|---|
| **`std.process` traits** (`Subtracting`, `Forming`, `Joining`, etc.) | Cannot classify processes or enforce DFM rules by category | Medium -- trait definitions exist in spec, need implementation. Already in task queue (#333). |
| **Temporal ordering beyond `chain`** | Cannot express "cure for 24 hours" or "cool to room temperature before next step" | Medium -- could model as occurrence parameters with duration constraints |
| **Conditional branching** | Cannot express "if tolerance > 0.01mm, add finishing pass" | Low-medium -- `where` guards on `sub` already exist, need extension to occurrences |
| **Resource/tool constraints** | Cannot express "only one person can use the table saw at a time" | Medium -- new constraint domain |
| **Feedback loops** | Cannot express "measure -> adjust -> remeasure" | Hard -- fundamental evaluation model change |

### 4.4 The Temporal Gap

The temporal gap is the most important one for OSE. Housing builds are fundamentally sequential with hard time dependencies (concrete cure time, paint dry time, inspection hold points). Reify's `chain` captures the ordering but not the duration or conditions.

The parameters and constraints for duration are expressible today:

```
occurrence def ConcretePour {
    port forms : in FormworkPort
    port cured : out StructuralPort
    param cure_duration : Duration = 7days
    param min_cure_temp : Temperature = 10degC

    constraint cure_duration >= 3days
}
```

What is missing is runtime awareness that `cure_duration` implies a temporal dependency on the next step. Currently it is just a number, not a scheduling constraint. This could be addressed by extending the constraint solver to recognize duration parameters on occurrences as scheduling edges, or by introducing a `std.process.Schedule` type that aggregates occurrence durations along chains.

### 4.5 Coverage Assessment

Reify's process modeling covers approximately 70% of what OSE needs today and has a clear path to the remaining 30%. The existing capabilities (occurrence definitions, port contracts, constraint propagation, chain composition, determinacy checking) are exactly the foundation that a rigorous build procedure specification requires. The gaps (temporal ordering, resource scheduling, conditional branching) are extensions to an already-suitable model, not fundamental redesigns.

---

## 5. Synthesis: The Integration Opportunity

The most interesting synergy: Iconic CAD's vision of "non-engineers designing infrastructure" + Reify's `auto` resolution + LLM co-authoring = a citizen designer places icons, the constraint solver fills in engineering parameters, and an LLM explains what it chose and why.

This is more rigorous than "AI parses your drawing" and more accessible than "write parametric CAD code."

### 5.1 What Each Side Brings

| Iconic CAD brings | Reify brings |
|---|---|
| UX vision for citizen designers | Formal semantics and constraint solving |
| Real-world build experience (Seed Eco-Home) | Compiler-enforced correctness guarantees |
| 20-trade domain knowledge and part libraries | Parametric module system with traits and ports |
| Community of builders and replicators | LLM co-authoring infrastructure |
| Validated icon-based design methodology | Determinacy checking ("is this design complete?") |

### 5.2 Path Forward

1. **Near-term** (language + stdlib): The `std.process` trait hierarchy, IO/Buy traits, tolerancing, and material traits are already in the task queue (31 stdlib tasks). At current throughput these will land within days, not weeks.
2. **Medium-term** (GUI): Build icon palette + canvas panel with bidirectional sync to `.ri` source; integrate with existing 3D viewport and property editor. ~85-120 tasks, estimated at 2-4 days of orchestrator time after PRD decomposition.
3. **Longer-term** (ecosystem): Port OSE's Seed Eco-Home module library to `.ri`; extend process modeling with temporal constraints; build catalog browsing and search tooling.

---

## Appendix A. Iconic CAD Overview

*For readers unfamiliar with the Iconic CAD project.*

Iconic CAD is a design paradigm from Marcin Jakubowski's [Open Source Ecology](https://www.opensourceecology.org/) (OSE) project. It is not a standalone CAD application -- it is a workflow/protocol layer on top of FreeCAD + AI for democratizing infrastructure design.

The core idea: drag-and-drop icons that are each linked to fully parametric, production-grade CAD models. A non-engineer arranges icons visually (e.g. in a graphics editor), and then AI parses the drawing and produces real CAD -- fabrication-ready 3D models, BOMs, technical drawings, build procedures, cost estimates, and more.

### A.1 Pipeline

1. **Part Libraries** -- open-source icon collections representing real CAD components (SVGs editable in any graphics program)
2. **Design Guides** -- open instructions for technically correct design
3. **Visual Assembly** -- drag-and-drop icons into a layout document
4. **AI Conversion** -- AI interprets the icon layout and generates real parametric CAD in FreeCAD

### A.2 Generated Outputs

Beyond 3D models, the pipeline outputs: bills of materials, technical drawings, build procedures, calculations (weight, strength, thermal), exploded diagrams, 3D print files, cost/labor estimates, and building department submission packages.

### A.3 Technical Architecture

- **4-layer schema**: generative schemas -> component libraries -> assembly schemas -> parametric system generators
- **Validation pipeline**: syntactic -> semantic -> stress testing across parameter spaces -> edge case -> documentation verification
- **Naming taxonomy**: `moduletype_part_attributes.ext` across ~20-30 trades (foundation, framing, electrical, plumbing, etc.)
- Built on **FreeCAD** as the CAD engine (which itself uses OpenCASCADE)

### A.4 Scope

Initially applied to OSE's Seed Eco-Home project (~1,000 parts/modules/tools). The ambition is to cover all 50 GVCS machines and 20 trades with ~50 tools/materials each. The claimed compression is ~100x over manual CAD design timelines.

### A.5 Maturity

Early-stage prototyping. The workflow specification was last updated March 17, 2026. It is being prototyped at Factor e Farm (OSE's 30-acre site in Maysville, MO). The wall module workflow was developed in ~1 month as a proof of concept. There is no downloadable tool or public software release -- it is a documented protocol with wiki pages describing the vision, workflow spec, and automation approach. The AI conversion step appears to be the least mature component.

### A.6 Sources

- [Iconic CAD -- OSE Wiki](https://wiki.opensourceecology.org/wiki/Iconic_CAD)
- [Iconic CAD Workflow](https://wiki.opensourceecology.org/wiki/Iconic_CAD_Workflow)
- [Iconic CAD Automation](https://wiki.opensourceecology.org/wiki/Iconic_CAD_Automation)
- [Iconic CAD Workflow Specification](https://wiki.opensourceecology.org/wiki/Iconic_CAD_Workflow_Specification)
- [YouTube: Iconic CAD - FreeCAD + AI Workflow for Distributed Housing Design](https://youtube.com/watch?v=N5ZEkXWSQHo) (Marcin Jakubowski / @marcinose)
