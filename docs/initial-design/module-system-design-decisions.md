# Module System: Design Decisions

**Status:** Foundation complete — ready for deferred syntax items and standard library boundary  
**Version:** 0.1 — First crystallization from module system design sessions  
**Builds on:** `ontology-design-decisions.md` v0.1, `syntax-design-decisions.md` v0.1, `name-resolution-and-scoping-design-decisions.md` v0.1

---

## 1. Design approach

The module system governs how Reify source files relate to each other: how declarations are organised into units, how those units control their public surfaces, and how they reference each other's definitions. The design prioritises predictability, LLM co-authorability, and alignment with the existing scoping and visibility rules established in the name resolution phase.

**Core principle:** The module system introduces one new concept — the module boundary — and one new mechanism — `pub` visibility. Everything else (intra-scope visibility rules, `self`, upward/downward access) is unchanged. Crossing a module boundary should feel like crossing any other scope boundary, with the single addition of an explicit visibility gate.

---

## 2. Language name and file extension

The language is named **Reify**. Source files use the `.ri` extension.

**Rationale:** "Reify" means "to make real" — the abstract-to-concrete journey that is the language's central activity. A design begins as an undetermined specification and is progressively reified into a fully determined, manufacturable artefact. The name describes exactly what the language does. It is concise, pronounceable, highly searchable, and has no collisions in the engineering domain. The two-character extension `.ri` is clean and unambiguous.

---

## 3. File–module mapping

### 3.1 One file = one module

Every `.ri` file is exactly one module. A file cannot contain multiple module declarations, and a module cannot span multiple files.

**Rationale:** A strict one-to-one mapping means the file system is the module system. There is no ambiguity about where a definition lives, no tooling complexity around multi-file modules, and no confusion when reading a file in isolation (e.g. in a diff, a review, or an LLM context window). Engineers already organise projects hierarchically — products contain assemblies contain components — and this maps naturally to directory trees.

### 3.2 Mandatory full-path module declaration

Every `.ri` file must begin with a `module` declaration specifying its full path:

```
module std.mechanical.fasteners.bolt
```

The declared path must match the file's location in the source tree. The file above must be located at `std/mechanical/fasteners/bolt.ri`. This correspondence is enforced by tooling.

**Rationale:** The full path is self-documenting — reading the file tells you unambiguously where it sits in the module hierarchy. The redundancy with the filesystem is intentional: it acts as a consistency check, and it ensures that moving a file forces an explicit acknowledgment of the identity change. This is a feature, not a cost — every `import` referencing the module also needs updating, so the file edit is the least of the refactoring work, and tooling (including LLMs) can automate it trivially.

### 3.3 Directories are namespaces

A directory in the source tree is a namespace, not a module. It has no declarations of its own and cannot be imported directly for its contents.

A directory may contain a `mod.ri` file, which serves as the directory-level module. Its primary purpose is curating the directory's public API via re-exports:

```
// std/mechanical/fasteners/mod.ri
module std.mechanical.fasteners

pub import std.mechanical.fasteners.bolt.Bolt
pub import std.mechanical.fasteners.nut.Nut
pub import std.mechanical.fasteners.washer.Washer
```

This allows consumers to write `import std.mechanical.fasteners.Bolt` rather than `import std.mechanical.fasteners.bolt.Bolt`, providing a clean package-level API without exposing internal file organisation.

**Rationale:** Separating namespaces (directories) from modules (files) avoids the complexity of directory-as-module systems where it is unclear whether a directory can contain both declarations and child modules. The `mod.ri` convention (following Rust's precedent) is a well-understood pattern that solves the API curation problem without special language machinery.

---

## 4. Visibility at module boundaries

### 4.1 Two-level visibility model

Visibility is governed by two orthogonal mechanisms:

1. **`pub` on definitions** — gates whether a definition is visible outside its module at all.
2. **Intra-scope visibility rules** — gate which members of a visible definition are accessible.

These answer different questions. `pub` answers "can you see this definition?" — a module-level concern. The intra-scope rules answer "given that you can see it, what's accessible?" — a type-level concern established in the name resolution phase.

### 4.2 Module-level visibility: `pub`

Definitions are module-private by default. Only definitions marked `pub` are visible outside the module:

```
pub structure def Bracket { ... }       // Visible outside module
structure def InternalHelper { ... }    // Module-private
```

**Rationale:** Explicit public API surfaces make library design intentional. The default-private convention (following Rust) ensures that internal implementation details do not accidentally become part of a module's contract.

### 4.3 Member-level visibility: intra-scope rules apply

Once a definition is visible (because it is `pub`), the standard intra-scope visibility rules from the name resolution phase apply:

- **Parameters** and **named occurrences** (including ports) are visible from outside.
- **Local bindings** (`let` declarations) and **constraints** are private.

This means a `pub structure def Motor` exposes its params, ports, and named sub-structures to importers — exactly as if it were defined in the importer's local scope. No additional `pub` annotations on individual params are needed.

### 4.4 Member-level `pub` as escape hatch

The `pub` keyword may be applied to individual members to override the default visibility:

```
pub structure def Motor : RigidMechanical {
    param rated_torque : Torque                    // Visible (param default)
    param voltage : Voltage = 24V                  // Visible (param default)

    port shaft : RotaryPort                        // Visible (named occurrence default)

    stator : Stator { ... }                        // Visible (named occurrence default)

    let winding_resistance : Resistance = ...      // Private (local default)
    pub let torque_constant : Torque/Current = ...  // Visible (pub override)

    require rated_speed <= 10000 rpm               // Private (constraint default)
}
```

**Rationale:** Occasionally a computed local value is part of the meaningful public interface (e.g. a derived constant that consumers need for constraint composition). `pub let` exposes it without promoting it to a parameter.

### 4.5 No `priv` modifier in v0.1

There is no mechanism to hide a parameter of a `pub` definition. If a definition is public, all its parameters are visible. If a value should not be visible, make it a `let` binding instead.

**Rationale:** The use case for hidden parameters on public definitions is weak — a parameter exists precisely to be configured from outside. Hiding one contradicts its purpose. If experience reveals genuine need, `priv` can be introduced later without breaking existing code.

---

## 5. Import semantics

### 5.1 Import forms

```
import std.mechanical.fasteners              // Module import — qualified access
import std.mechanical.fasteners.Bolt         // Entity import — unqualified access
import std.mechanical.fasteners.{Bolt, Nut}  // Destructured import — multiple entities
import std.mechanical.fasteners as f         // Module alias
import std.mechanical.fasteners.Bolt as StdBolt  // Entity rename
```

### 5.2 What is importable

Anything marked `pub` at the top level of a module is importable: structure definitions, occurrence definitions, constraint definitions, field definitions, trait definitions, and `pub let` bindings.

### 5.3 Module import gives qualified access

Importing a module introduces its name (or alias) into scope, providing access to its `pub` members via dot notation:

```
import std.mechanical.fasteners

// Access via module name:
my_bolt : fasteners.Bolt { ... }
```

Importing a specific entity introduces it unqualified:

```
import std.mechanical.fasteners.Bolt

// Direct access:
my_bolt : Bolt { ... }
```

### 5.4 Full qualified paths always work

A fully qualified path resolves regardless of whether the module has been imported. Imports introduce shorter names — they do not gate access:

```
// No import needed:
my_bolt : std.mechanical.fasteners.Bolt { ... }
```

**Rationale:** This eliminates "must import before use" friction and ensures that any reference in source text is self-describing. LLM-generated code can use full paths without worrying about which imports are present, and tooling can add shortening imports as a cleanup step.

### 5.5 Aliases are additive

An alias introduces an additional name. It does not replace the original:

```
import std.mechanical.fasteners as f

// Both work:
my_bolt : f.Bolt { ... }
my_nut : std.mechanical.fasteners.Nut { ... }
```

### 5.6 No wildcard imports

`import std.mechanical.fasteners.*` is not supported. All imports are explicit.

**Rationale:** Wildcard imports create ambiguous name resolution, complicate LLM generation (which names are available?), and make dependencies opaque. The one exception is the prelude (§7), which is compiler-inserted and not user-writable.

### 5.7 Re-exports

A module can re-export an imported entity as part of its own public API:

```
pub import internal.helper.UsefulTrait
```

Re-exports are transparent — the entity appears as if it were defined in the re-exporting module. This is the primary mechanism for `mod.ri` package files to curate a directory's public surface.

---

## 6. Module dependency graph

### 6.1 No circular dependencies

The module dependency graph must be a directed acyclic graph (DAG). If module A imports module B (directly or transitively), module B cannot import module A.

**Rationale:** Circular module dependencies are a design smell indicating that the modules should be merged or that a shared dependency should be extracted. Forbidding cycles simplifies compilation order, incremental recompilation, and tooling. It is also consistent with the language's existing architecture — the evaluation graph is a DAG, the abstraction hierarchy is a DAG, the constraint dependency graph is a DAG. Modules follow the same pattern.

### 6.2 Practical consequence

If two definitions genuinely need mutual references, they belong in the same module. If a structure in module A references a structure in module B and vice versa, the common interface (a trait or abstract structure) should be extracted into a third module that both depend on. This is standard engineering of dependencies and results in cleaner, more maintainable designs.

---

## 7. Prelude

### 7.1 Implicit prelude import

Every module implicitly imports `std.prelude`. This is the single exception to the no-wildcard-imports rule. The user never writes this import; the compiler inserts it.

### 7.2 Prelude contents (v0.1)

The prelude contains declarations that are universally needed:

- **Primitive types:** `Bool`, `Int`, `Float`, `String`
- **Physical quantity types:** `Length`, `Angle`, `Mass`, `Time`, `Force`, `Torque`, `Pressure`, `Temperature`, `Velocity`, `AngularVelocity`, `Current`, `Voltage`, `Resistance`
- **Unit literals:** `mm`, `m`, `deg`, `rad`, `kg`, `s`, `N`, `Pa`, `rpm`, `V`, `A`, `°C`, and the core SI-derived set
- **Fundamental constants:** `pi`
- **Core traits:** to be finalised with the standard library boundary work

### 7.3 Prelude design principles

The prelude should be:

- **Small** — a designer should be able to memorise its contents.
- **Stable** — changes to the prelude break every module. Additions are acceptable; removals and semantic changes are not.
- **Universal** — everything in the prelude should be useful to a significant majority of modules. Domain-specific definitions (materials, fasteners, DFM rules) do not belong here.

### 7.4 Suppression

The pragma `#no_prelude` suppresses the implicit prelude import. This is needed for defining the prelude itself and for exotic contexts where the standard prelude is inappropriate.

---

## 8. Interaction with existing scoping rules

### 8.1 Imports do not create lexical parent scopes

Imported names enter the module's top-level namespace. They do not participate in upward visibility — an imported name is not a lexical parent of any definition within the module. A definition inside the module cannot "see" imported names via the child-sees-parent mechanism. Instead, imported names are simply available as names in the module scope, subject to the same resolution rules as local declarations.

**Rationale:** This falls out naturally from the existing scoping model. Upward visibility is defined in terms of lexical enclosure. Imports are not lexical enclosures — they are name introductions.

### 8.2 `self` is unaffected by modules

`self` refers to the enclosing entity definition, never to the module. The module is not an entity — it is an organisational unit — and `self` has no meaning at module scope.

### 8.3 Order-independence within modules

All declarations within a module are mutually visible regardless of textual order, consistent with the order-independence rule from the name resolution phase. Import statements are conventionally placed at the top of the file but are not required to precede the declarations that use them.

**Rationale:** The language is declarative. Requiring imports before use would impose an artificial sequencing constraint inconsistent with the established order-independence principle.

---

## 9. Open questions for subsequent design phases

### 9.1 Standard library boundary

What ships in `std` vs. what belongs in community libraries? The prelude contents (§7.2) are a starting point, but the full `std` tree — mechanical primitives, electrical primitives, materials, manufacturing traits — requires its own design pass.

### 9.2 Versioning and compatibility

Module versioning, dependency resolution, and compatibility guarantees are not addressed in v0.1. The module system as designed is compatible with external dependency management tools (analogous to Cargo, pip, or Go modules) but does not prescribe one.

### 9.3 Conditional compilation

Conditional imports or platform-specific module variants are deferred. The `where` mechanism applies to entity declarations within modules, not to modules themselves.

---

## 10. Summary of key decisions

| Decision | Choice | Rationale |
|---|---|---|
| Language name | Reify (`.ri` extension) | "To make real" — describes the abstract→concrete design journey; concise, searchable, no domain collisions |
| File–module mapping | One file = one module, strictly | Predictable, LLM-friendly, eliminates ambiguity |
| Module declaration | Mandatory full path, must match filesystem | Self-documenting, consistency check, forces acknowledgment of identity changes |
| Directories | Namespaces, not modules; `mod.ri` for API curation | Clean separation; avoids directory-as-module complexity |
| Definition visibility | `pub` keyword, default-private | Explicit public APIs; Rust convention |
| Member visibility | Intra-scope rules unchanged; `pub` on members as escape hatch | Orthogonal to module visibility; params and named occurrences visible by default |
| No `priv` in v0.1 | Public definitions expose all params | Hidden params contradict parameter purpose; defer until usage patterns justify |
| Import forms | Explicit, destructured, aliased, renamed — no wildcards | Unambiguous resolution; LLM-friendly; transparent dependencies |
| Full qualified paths | Always work, imports are additive shortcuts | Self-describing references; eliminates import-before-use friction |
| Re-exports | `pub import` — transparent | Enables `mod.ri` package curation |
| Circular dependencies | Forbidden — module graph is a DAG | Consistent with eval graph; simplifies compilation; design smell |
| Prelude | Implicit `std.prelude`, small, stable, suppressible via `#no_prelude` | Universal definitions available everywhere; single controlled exception to no-wildcards |
| Import scoping | No lexical parent relationship; `self` unaffected | Falls out from existing scoping rules; no new machinery |
| Order-independence | Declarations and imports order-independent within a module | Consistent with established declarative order-independence |

---

*Document generated from module system design sessions. Intended as a living specification to be refined through standard library boundary definition and end-to-end worked examples.*
