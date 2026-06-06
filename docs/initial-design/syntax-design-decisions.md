# Syntax Design: Design Decisions

**Status:** First breadth-first draft — all layers covered, ready for iteration  
**Version:** 0.1 — First crystallization from syntax design sessions  
**Builds on:** `ontology-design-decisions.md` v0.1, `type-system-design-decisions.md` v0.1

---

## 0. Guiding syntax principles

The syntax must serve two audiences simultaneously: human engineers reading/writing design intent, and LLMs generating/consuming structurally regular code. These goals are more aligned than they might appear — both benefit from low ambiguity, consistent patterns, and explicit structure.

**Principles (in priority order):**

1. **Regularity** — Every entity type follows the same declaration shape. Every member kind uses the same syntax pattern. Minimise special forms.
2. **Concision** — Common things should be short. Rare things can be longer. No ceremony for simple cases.
3. **Explicitness** — Structure is visible in the text. No significant whitespace. No implicit scoping rules that require counting indentation levels.
4. **Readability at a glance** — A human scanning a file should immediately see what entities exist, what their types are, and what connects to what. Keywords over symbols for semantically important constructs.
5. **Parseability** — LL(1) or close to it. No ambiguities that require unbounded lookahead. LLMs generate more reliably when grammar is predictable.

**Style family:** Curly-brace, declarative, Modelica-meets-USD as established in the ontology. Rust-influenced expression syntax. Not C++, not Python, not Lisp.

---

## 1. Lexical foundations

### 1.1 Identifiers

```
snake_case      — values, parameters, ports, sub-structures, fields, modules
PascalCase      — types, traits, entity definitions
SCREAMING_SNAKE — compile-time constants (convention, not enforced by grammar)
```

Identifiers start with a letter or underscore, followed by letters, digits, and underscores. Unicode letters are permitted (for internationalisation of parameter names, material names, etc.), but keywords are ASCII-only.

### 1.2 Comments

```
// Line comment — to end of line
/* Block comment — nests correctly */
/// Doc comment — attached to the following declaration
```

Block comments nest (unlike C). `/* outer /* inner */ still in outer */` is valid. This is a small implementation cost for significant usability gain (commenting out code that already contains comments).

### 1.3 Numeric literals

```
42              // Int
3.14            // Real
1.5e-3          // Real, scientific notation
1_000_000       // Underscores as visual separators (Int or Real)
0xFF            // Hex integer
0b1010          // Binary integer
```

No implicit coercion from `Real` to `Int`. `Int` promotes to `Real` implicitly (as specified in the type system doc).

### 1.4 String literals

```
"hello world"           // Standard string
"line one\nline two"    // Escape sequences: \n \t \\ \" \uXXXX
```

No string interpolation in the core language. String interpolation is a display/templating concern, not a design language concern. If needed, it belongs in a formatting library.
> **Superseded in v0.6** — string interpolation is now a core-language feature; see docs/prds/v0_6/string-interpolation.md and reify-language-spec.md §2.4.

### 1.5 Boolean literals

```
true    false
```

### 1.6 Special value literals

```
undef   // Undefined — "not yet decided"
auto    // Delegated — "system, figure this out"
```

These are keywords, not identifiers. They can appear anywhere a value expression is expected.

---

## 2. Physical quantity literals and units

### 2.1 Quantity literals

A physical quantity is a numeric literal immediately followed by a unit expression. No space between number and unit:

```
5mm                 // Length: 5 millimetres
3.2kN               // Force: 3.2 kilonewtons
45deg               // Angle: 45 degrees
293.15K             // Temperature: 293.15 kelvin
2.5e-3m             // Length: 0.0025 metres (scientific notation)
```

**No space between number and unit.** This is a firm decision. `5mm` is a quantity literal. `5 mm` is the integer `5` followed by the identifier `mm` — a syntax error or misinterpretation. Rationale: unambiguous tokenisation; matches how engineers actually write quantities; prevents the parser from needing to decide whether an identifier after a number is a unit or a variable name.

**Bare numbers are dimensionless.** `3.14` is `Real` (dimensionless). To get a dimensioned quantity, you must write a unit. There is no "default unit system."

### 2.2 Unit expressions

Units compose with `*`, `/`, and `^` in postfix position after a number:

```
5kN*m               // Torque: kilonewton-metres
2.1kg/m^3           // Density
9.81m/s^2           // Acceleration
1.2e-6m^2/s         // Kinematic viscosity
```

**Precedence within unit expressions:** `^` binds tightest, then `*` and `/` left-to-right. Parentheses are available for disambiguation:

```
5kg*m/s^2           // = kg·m·s⁻²  (Force) — ^ binds to s only
5kg*m/(s^2)         // Same thing, explicit
5(kg*m/s)^2         // = kg²·m²·s⁻² — parenthesised unit raised to power
```

### 2.3 Unit prefix table (built-in)

SI prefixes from `q` (quecto, 10⁻³⁰) through `Q` (quetta, 10³⁰). Common base units:

| Dimension | Base units |
|---|---|
| Length | `m`, `mm`, `um`, `nm`, `km`, `in`, `ft`, `thou` |
| Mass | `kg`, `g`, `mg`, `lb`, `oz` |
| Time | `s`, `ms`, `us`, `ns`, `min`, `hr` |
| Current | `A`, `mA`, `uA` |
| Temperature | `K`, `degC`, `degF` |
| Angle | `rad`, `deg`, `arcmin`, `arcsec`, `rev` |
| Force | `N`, `kN`, `lbf` |
| Pressure | `Pa`, `kPa`, `MPa`, `GPa`, `psi`, `bar` |
| Energy | `J`, `kJ`, `eV` |
| Power | `W`, `kW`, `MW` |
| Voltage | `V`, `mV`, `kV` |

The full unit table is defined in the standard library, not the grammar. The grammar defines the syntax for unit expressions; the standard library populates the unit namespace.

### 2.4 Temperature offsets

`degC` and `degF` are offset units. The distinction between absolute temperature and temperature difference matters:

```
param max_temp : Temperature = 150degC       // Absolute: 423.15 K
param delta_t  : TemperatureDiff = 20degC     // Difference: 20 K

// Type system distinguishes these:
// Temperature + TemperatureDiff → Temperature     ✓
// Temperature - Temperature → TemperatureDiff     ✓
// Temperature + Temperature → type error          ✗
```

This mirrors the Point/Vector distinction for temperatures. `K` is both absolute and difference (since kelvin has no offset). `degC` and `degF` literals have context-dependent interpretation based on the declared dimension.

### 2.5 Ranges

```
2mm..5mm            // Range<Length>: closed interval [2mm, 5mm]
0deg..<360deg       // Half-open: [0°, 360°)
>2mm                // Open lower bound: (2mm, ∞)
<=100MPa            // Closed upper bound: (-∞, 100MPa]
```

Range syntax uses `..` (inclusive) and `..<` (exclusive upper bound). Single-sided ranges use comparison operators as prefixes.

---

## 3. Type expressions

### 3.1 Named types

```
Length                          // Simple named type (dimension alias)
Bolt                            // Structure type
List<Point3<Length>>            // Generic type with nested parameters
Tensor<2, 3, Pressure>         // Mixed type and value parameters
```

### 3.2 Trait bounds

In type parameter positions:

```
<T: Fastener>                   // T must implement Fastener
<T: Rigid + Conductive>         // Multiple trait bounds
<N: Nat>                        // Kind bound
<Q: Dimension>                  // Dimension kind
```

### 3.3 Dimension expressions

Used in `where` clauses and dimension alias definitions:

```
dimension Pressure = Force / Area
dimension Stiffness = Force / Length
dimension Density = Mass / Length^3
```

Dimension expressions use `*`, `/`, `^` with the same precedence as unit expressions.

### 3.4 Function types

For fields and constraint definitions:

```
Point3<Length> -> Scalar<Temperature>       // Spatial temperature field
(Length, Length) -> Bool                     // Binary predicate
```

Arrow `->` for function types. Tuple types with `(A, B, C)` for multi-parameter domains.

---

## 4. Entity declarations

### 4.1 General shape

All entity declarations follow a uniform pattern:

```
<entity_kind> def <Name><TypeParams>? <TraitList>? <WhereClause>? {
    <members>
}
```

Where `<entity_kind>` is one of: `structure`, `occurrence`, `constraint`, `field`.

### 4.2 Structure declarations

```
structure def Bracket<M: Material> : RigidMechanical {
    param thickness : Length
    param width : Length = 50mm
    param material : M

    port mount_face : MechanicalPort {
        direction = in
        frame = Frame3 { origin = point(0mm, 0mm, 0mm) }
    }

    port load_face : MechanicalPort {
        direction = out
        frame = Frame3 { origin = point(0mm, width, 0mm) }
    }

    sub rib : Rib { height = thickness * 0.8 }

    derived volume : Volume = thickness * width * width
    derived mass : Mass = volume * material.density

    constraint thickness > 1mm
    constraint thickness < width / 2
}
```

**Member keywords:**

| Keyword | Purpose | Notes |
|---|---|---|
| `param` | Value parameter | Can be `undef`, constrained, `auto`, or determined |
| `port` | Interaction point | Typed; contains own parameters, frame, constraints |
| `sub` | Contained sub-structure | Named child in containment tree |
| `derived` | Computed value | Expression over other members; overridable |
| `constraint` | Inline constraint | Anonymous predicate that must hold |

**`param` not `parameter`:** The keyword is shortened because it is by far the most frequently written member kind. The other member keywords are already short (`port`, `sub`, `derived`). One canonical spelling — no `parameter` alias. Regularity over accommodation.

### 4.3 Occurrence declarations

```
occurrence def Welding : FusionJoining {
    param method : WeldMethod
    param filler : Material = auto

    port workpiece_a : in StructurePort
    port workpiece_b : in StructurePort
    port result : out StructurePort

    param current : Current
    param voltage : Voltage
    param travel_speed : Length / Time

    derived heat_input : Energy / Length = (current * voltage) / travel_speed

    constraint heat_input < workpiece_a.material.max_heat_input
}
```

Same member keywords. `in`/`out` on ports express flow direction (material in, result out). Occurrence ports carry structures, not geometric interfaces.

### 4.4 Constraint declarations

```
constraint def MinWallThickness {
    param wall : Length
    param process : ManufacturingProcess

    wall >= process.min_wall_thickness
}

constraint def Coaxial {
    param a : CylindricalFeature
    param b : CylindricalFeature

    distance(a.axis, b.axis) == 0mm
    angle(a.axis.direction, b.axis.direction) == 0deg
}
```

Constraints use the same body-level `param` declarations as all other entity types. This preserves the uniform declaration shape across all four entity kinds, and enables `derived` members inside complex constraints — important for readability in DFM rule libraries and other non-trivial constraint definitions.

The body contains `param` and `derived` declarations intermixed with predicate expressions. Bare expressions in a constraint body are assertions — lines without a member keyword are predicate lines. Default connective between predicate lines is `and` (conjunction).

**Logical connectives in constraint bodies:**

```
constraint def ValidBoltSelection {
    param bolt : Bolt
    param load : Force

    bolt.rated_load >= load * 1.5
    bolt.shank_diameter >= 4mm
    bolt.material.yield_strength >= 200MPa

    // Disjunction
    bolt.head_type == HeadType.Hex or bolt.head_type == HeadType.Socket

    // Implication
    load > 10kN implies bolt.grade >= 10.9

    // Quantification (over a collection parameter)
    // forall hole in bolt_holes: hole.diameter == bolt.shank_diameter + 0.5mm
}
```

**Complex constraints benefit from `derived` intermediates:**

```
constraint def DFM_Milling {
    param part : Structure
    param machine : MillingMachine
    param tolerance : Length = 0.05mm

    derived min_radius : Length = machine.min_tool_radius
    derived max_aspect : Real = machine.max_wall_aspect_ratio

    forall feature in part.internal_corners:
        feature.radius >= min_radius
    forall wall in part.walls:
        wall.thickness >= wall.depth / max_aspect
    part.max_depth <= machine.z_travel
}
```

Default connective between lines in a constraint body is `and` (conjunction). `or`, `not`, `implies`, `forall`, `exists` are keywords.

### 4.5 Field declarations

```
field def temperature_distribution : Point3<Length> -> Scalar<Temperature> {
    source = analytical {
        |p| 300K + 50K * exp(-distance(p, heat_source) / 10mm)
    }
}

field def material_density : Point3<Length> -> Scalar<Density> {
    source = sampled {
        grid = RegularGrid3 { spacing = 0.5mm, bounds = part.bounding_box }
        interpolation = trilinear
        data = import("density_field.vdb")
    }
}

field def composite_stiffness : Point3<Length> -> Tensor<2, 3, Pressure> {
    source = composed {
        |p| if region_a.contains(p) then stiffness_a(p)
            else if region_b.contains(p) then stiffness_b(p)
            else base_stiffness
    }
}
```

Fields have a **domain → codomain** type signature and a **source** specifying how the field is defined. Source kinds:

| Source kind | Meaning |
|---|---|
| `analytical` | Closed-form expression, given as a lambda |
| `sampled` | Discrete samples on a grid/mesh, with interpolation |
| `composed` | Combination of other fields via arithmetic, logic, or conditional |
| `imported` | External data file (OpenVDB, CSV, HDF5, etc.) |

The lambda syntax `|params| expression` follows Rust convention. Used for analytical field definitions and inline field expressions.

### 4.6 Instantiation

Instantiation creates an instance from a defined type:

```
// Full form
bracket1 : Bracket<Steel_304> {
    thickness = 5mm
    width = 80mm
}

// Minimal — type only, all parameters undef
bracket2 : Bracket<Steel_304>

// With auto
bracket3 : Bracket<auto> {
    thickness = 5mm
    width = auto
}

// Anonymous (inline, within a parent structure)
sub mount : Bracket<Aluminium_6061> { thickness = 3mm }
```

**Instantiation syntax:** `name : Type { param = value, ... }`. The curly-brace block is optional if no parameters are overridden. This mirrors the definition syntax but without the `def` keyword.

**The colon `:` means "is a."** In definitions, it introduces trait conformance. In instantiations, it introduces the type. Consistent reading: `bracket1 : Bracket<Steel_304>` reads "bracket1 is a Bracket of Steel_304."

---

## 5. Expressions

### 5.1 Arithmetic

```
a + b       a - b       a * b       a / b
a ^ n       -a          a % b       // modulo (integers only)
```

All arithmetic is dimensionally checked. `5mm + 3mm` = `8mm`. `5mm + 3kg` = type error. `5mm * 3mm` = `15mm^2`. Standard precedence: `^` > unary `-` > `*` `/` `%` > `+` `-`.

### 5.2 Comparison

```
a == b      a != b
a < b       a > b       a <= b      a >= b
```

Equality is structural for value types, identity for structure references. Comparisons are dimensionally checked.

### 5.3 Logical

```
a and b     a or b      not a
a implies b
forall x in collection : predicate(x)
exists x in collection : predicate(x)
```

Keywords, not symbols. Rationale: `&&` and `||` are C idioms that add no clarity; keyword forms are unambiguous and read naturally in engineering constraint contexts.

**Precedence:** `not` > `and` > `or` > `implies`. Parentheses for disambiguation.

### 5.4 Conditional

```
if condition then expr_a else expr_b
```

Expression-level conditional (always has both branches, always produces a value). Not a statement-level `if`. Usable in field definitions, derived parameter expressions, constraint bodies.

### 5.5 Lambda

```
|x| x * 2
|p : Point3<Length>| distance(p, origin)
|a, b| a.thickness + b.thickness
```

Lambdas are anonymous functions. Parameter types are inferred where possible, annotatable where needed. Primary use: analytical field definitions, inline predicates, higher-order constraint helpers.

### 5.6 Member access

```
bracket.thickness               // Parameter access
bracket.mount_face              // Port access
bracket.mount_face.frame        // Nested access
bracket.rib.height              // Sub-structure member access
```

Dot-notation for member access. Chained access resolves through the containment tree.

### 5.7 Qualified trait access

```
Fastener::rated_load            // Disambiguate when two traits define same name
bracket.(Rigid::max_temperature)  // Instance-level qualified access
```

`Trait::member` for type-level disambiguation. Parenthesised qualified access on instances when needed.

### 5.8 Collection expressions

```
[1, 2, 3]                       // List literal
set{a, b, c}                    // Set literal — explicit prefix avoids block ambiguity
("key" => value, "k2" => v2)    // Map literal

list.map(|x| x * 2)             // Map over collection
list.filter(|x| x > 5mm)        // Filter
list.fold(0mm, |acc, x| acc + x)  // Fold/reduce
list.all(|x| x > 0mm)           // Universal quantifier (returns Bool)
list.any(|x| x > 100mm)         // Existential quantifier (returns Bool)
list.count                       // Size
list.sum                         // Sum (for numeric collections)
```

Set literals use `set{...}` prefix to avoid ambiguity with blocks.

### 5.9 `undef` and `auto` in expressions

```
param thickness : Length = undef     // Explicit undef (overrides default)
param width : Length = auto          // Delegated to solver

// undef propagation
derived area : Area = thickness * width  // Area is undef if either input is undef
```

`undef` and `auto` are valid in any expression position where a value is expected. They interact with the determinacy tracking system as specified in the ontology doc.

---

## 6. Connection syntax

### 6.1 Basic connection

```
connect motor.shaft -> coupling.driver
connect coupling.driven -> gearbox.input : SplineConnection { tooth_count = 24 }
```

`connect` is a statement-level construct. `->` indicates the connection direction (read: "connects to"). The connector type and parameters follow `:` (optional — defaults to `auto`).

### 6.2 Bidirectional and reverse

```
connect plate_a.face -> plate_b.face : ButtWeld      // Directional
connect plate_a.face <-> plate_b.face : ButtWeld      // Explicitly bidirectional
```

`<->` is available for explicitly bidirectional connections. `->` with two `bidi` ports is equivalent but `<->` makes the bidirectionality visible in the source text.

### 6.3 Connector parameterisation

```
connect housing.bore -> shaft.journal : ShrinkFit {
    interference = 0.02mm
    assembly_temperature_delta = 150degC
}

connect flange_a.face -> flange_b.face : M8Bolt {
    grade = 10.9
    count = 6
    pattern = BoltCircle { diameter = 80mm }
}
```

The connector block `{ ... }` follows the same syntax as instantiation parameter blocks.

### 6.4 Ad-hoc connections

```
// Minimal — topology only
connect phone_case -> nut

// With connector type
connect phone_case -> nut : Adhesive { type = JBWeld }

// With ad-hoc geometry — @ operator designates a region on a structure
connect bracket@face(top_surface) -> plate@face(bottom_surface) : Adhesive
connect pipe@region(outer_surface, z = 0mm..50mm) -> clamp@region(inner_surface)
```

**The `@` operator** creates an ad-hoc port on a structure by designating a geometric region. Syntax: `structure@selector(arguments)`.

Selectors (standard library, not grammar-level):

| Selector | Meaning |
|---|---|
| `@face(name_or_expr)` | A named or computed surface |
| `@region(surface, constraints...)` | A sub-region of a surface |
| `@point(coordinates)` | A specific point |
| `@edge(name_or_expr)` | A named or computed edge |
| `@body(name_or_expr)` | A named or computed volume region |

### 6.5 Interface-level connections

```
// Connect all ports of a matching interface pair
connect motor.nema17 -> mount_plate.nema17 : NEMA17BoltSet { grade = 8.8 }

// Explicit port mapping when interfaces differ
connect motor.nema17 -> adapter.side_a {
    shaft -> input_bore
    bolt_hole_1 -> mounting_a
    bolt_hole_2 -> mounting_b
}
```

When both sides implement the same interface trait, ports are matched by name. When interfaces differ, an explicit mapping block lists the correspondences.

### 6.6 Sequential composition (occurrence chaining)

```
// Process chain: each step's output feeds the next
connect casting.result -> machining.workpiece
connect machining.result -> heat_treat.workpiece
connect heat_treat.result -> finishing.workpiece

// Chained shorthand
chain casting -> machining -> heat_treat -> finishing
```

`chain` is sugar for a sequence of `connect` statements connecting each occurrence's default output port to the next occurrence's default input port. Explicit `connect` is always available for non-linear or multi-port process flows.

---

## 7. Block structure and declaration ordering

### 7.1 Top-level declarations

A source file contains a sequence of top-level declarations:

```
module my_designs

import std.mechanical.fasteners.{Bolt, Nut, Washer}
import std.materials.metals.{Steel_304, Aluminium_6061}
import company.standards.{MinWallThickness, DFM_Milling}

structure def Widget : RigidMechanical { ... }

occurrence def WidgetMilling : Milling { ... }

constraint def WidgetDFM(w : Widget) { ... }
```

**Declaration order does not matter.** All names within a module are visible throughout the module regardless of textual position. Rationale: engineers organise files thematically, not dependency-order. Forward references are natural and expected.

### 7.2 Nesting

Structure definitions can nest:

```
structure def Gearbox : RigidMechanical {
    structure def Stage {
        param ratio : Real
        sub gear_a : Gear
        sub gear_b : Gear
        constraint gear_b.teeth == gear_a.teeth * ratio
    }

    sub stage_1 : Stage { ratio = 3.5 }
    sub stage_2 : Stage { ratio = 2.8 }
}
```

Nested definitions are scoped to their parent. `Stage` outside `Gearbox` would require `Gearbox.Stage`. Nesting is purely a scoping/organisational mechanism — it does not imply containment (that's what `sub` does).

### 7.3 Member ordering within a block

No enforced ordering. Convention (for human readability):

1. Parameters
2. Ports
3. Sub-structures
4. Connections
5. Derived values
6. Constraints

Tooling may sort/group for display, but the grammar accepts any order.

---

## 8. Module system

### 8.1 Module declaration

```
module company.products.actuators
```

One module declaration per file, at the top. The module path corresponds to the file's location in the source tree (enforced by tooling, not grammar).

### 8.2 Imports

```
import std.mechanical.fasteners            // Import whole module
import std.mechanical.fasteners.Bolt       // Import single entity
import std.mechanical.fasteners.{Bolt, Nut, Washer}  // Import multiple
import std.mechanical.fasteners as fasteners  // Alias
import std.mechanical.fasteners.Bolt as StdBolt  // Rename
```

**No wildcard imports.** `import std.mechanical.fasteners.*` is not supported. Rationale: wildcard imports create ambiguous name resolution, complicate LLM generation (which names are available?), and make dependencies opaque.

### 8.3 Visibility

```
pub structure def Bracket { ... }           // Public — visible outside module
structure def InternalHelper { ... }        // Module-private (default)
```

**Default visibility is private.** Entities are module-private unless marked `pub`. This is the Rust convention. Rationale: explicit public API surfaces make library design intentional.

`pub` applies to entity definitions and to members within definitions:

```
structure def Motor : Actuator {
    pub port shaft : RotaryPort               // Accessible from outside
    param winding_resistance : Resistance  // Private implementation detail
}
```

### 8.4 Re-exports

```
pub import internal.helper.UsefulTrait      // Re-export from this module's public API
```

---

## 9. Annotations and pragmas

### 9.1 Annotations

```
@optimised("geo_kernel::coincidence_solver")
constraint def Coincident(a : Point3<Length>, b : Point3<Length>) {
    distance(a, b) == 0mm
}

@deprecated("Use RevisedBracket instead")
structure def OldBracket { ... }

@test
constraint def TestBoltStrength() { ... }
```

Annotations are metadata markers using `@name` or `@name(arguments)`. They do not change semantics — they provide hints to the toolchain.

**The `@optimised` annotation** is the hook mechanism referenced in the ontology doc (§2.3). It registers that a language-level definition has a semantically equivalent optimised implementation available in the runtime.

### 9.2 Pragmas

```
#precision(float64)             // Hint to toolchain about numeric precision
#solver(nlopt, algorithm = LD_SLSQP)  // Solver preference for constraints in scope
```

Pragmas use `#name(arguments)` and are scoped to the enclosing block. They are toolchain directives, not part of the semantic model. Pragmas never change the meaning of a program — only its implementation characteristics.

---

## 10. Putting it together: extended examples

### 10.1 A simple bracket

```
module examples.bracket

import std.mechanical.{RigidMechanical, MechanicalPort}
import std.materials.metals.Steel_304
import std.manufacturing.{DFM_Milling, MinWallThickness}

structure def LBracket : RigidMechanical {
    param thickness : Length = 5mm
    param width : Length = 50mm
    param height : Length = 75mm
    param material : Material = Steel_304

    port base_mount : MechanicalPort {
        direction = in
        frame = Frame3 { origin = point(0mm, 0mm, 0mm) }
    }

    port side_mount : MechanicalPort {
        direction = out
        frame = Frame3 { origin = point(0mm, 0mm, height) }
    }

    derived volume : Volume = thickness * width * (width + height)
    derived mass : Mass = volume * material.density

    constraint MinWallThickness(thickness, DFM_Milling)
    constraint width >= 3 * thickness
}
```

### 10.2 A bolted assembly

```
module examples.bolted_assembly

import std.mechanical.{RigidMechanical, MechanicalPort}
import std.mechanical.fasteners.{HexBolt, HexNut, FlatWasher}

structure def BoltedJoint : RigidMechanical {
    param bolt_size : ThreadSpec = M8
    param grip_length : Length

    sub top_plate : Plate { thickness = 10mm }
    sub bottom_plate : Plate { thickness = 12mm }

    sub bolt : HexBolt {
        thread = bolt_size
        length = auto                        // Solver determines from grip_length
        grade = 8.8
    }
    sub nut : HexNut { thread = bolt_size }
    sub washer_top : FlatWasher { bore = bolt_size.clearance_hole }
    sub washer_bottom : FlatWasher { bore = bolt_size.clearance_hole }

    connect top_plate.top_face -> washer_top@face(flat) -> bolt.head_bearing
    connect bottom_plate.bottom_face -> washer_bottom@face(flat) -> nut.bearing_face

    constraint grip_length == top_plate.thickness + bottom_plate.thickness
        + washer_top.thickness + washer_bottom.thickness
}
```

### 10.3 A manufacturing process chain

```
module examples.process_chain

import std.manufacturing.{Casting, CNCMilling, HeatTreatment, SurfaceFinish}

occurrence def BracketManufacturing {
    param material : CastableAlloy

    sub casting : SandCasting {
        alloy = material
        draft_angle = 3deg
    }

    sub rough_mill : CNCMilling {
        strategy = adaptive_clearing
        tool = EndMill { diameter = 12mm, flutes = 4 }
        tolerance = 0.1mm
    }

    sub finish_mill : CNCMilling {
        strategy = contour_parallel
        tool = EndMill { diameter = 6mm, flutes = 2 }
        tolerance = 0.02mm
    }

    sub heat_treat : HeatTreatment {
        process = stress_relief
        temperature = 550degC
        duration = 2hr
    }

    sub anodise : Anodising {
        type = TypeIII
        thickness = 25um
        colour = black
    }

    chain casting -> rough_mill -> finish_mill -> heat_treat -> anodise
}
```

### 10.4 A field-driven lattice structure

```
module examples.lattice_bracket

import std.fields.{spatial, interpolation}
import std.lattice.{GyroidCell, lattice_infill}
import std.analysis.{StressField, import_simulation_result}

structure def OptimisedBracket : RigidMechanical {
    param envelope : Solid             // Outer boundary
    param load_case : LoadCase         // Applied loads + BCs

    // Import stress analysis result as a field
    field stress : Point3<Length> -> Tensor<2, 3, Pressure> {
        source = imported { path = "bracket_stress.vtu", format = vtk }
    }

    // Derive a scalar density field from von Mises stress
    field relative_density : Point3<Length> -> Scalar<Dimensionless> {
        source = composed {
            |p| clamp(von_mises(stress(p)) / 100MPa, 0.15, 1.0)
        }
    }

    // Generate lattice infill driven by the density field
    sub infill : lattice_infill {
        cell = GyroidCell
        envelope = envelope
        density_field = relative_density
        min_wall = 0.4mm                    // Additive manufacturing constraint
    }
}
```

---

## 11. Grammar summary (semi-formal)

For reference. Not a complete formal grammar — intended to convey the shape of the language.

```
file            ::= module_decl? import* declaration*

module_decl     ::= 'module' module_path

import          ::= 'pub'? 'import' import_path ('as' IDENT)?
import_path     ::= module_path ('.' '{' IDENT (',' IDENT)* '}')?

declaration     ::= visibility? entity_decl | connect_stmt | chain_stmt

visibility      ::= 'pub'

entity_decl     ::= entity_kind 'def' TYPE_IDENT type_params? trait_list?
                     where_clause? '{' member* '}'

entity_kind     ::= 'structure' | 'occurrence' | 'constraint' | 'field'

type_params     ::= '<' type_param (',' type_param)* '>'
type_param      ::= TYPE_IDENT (':' trait_bound)? ('=' type_expr)?
                   | IDENT ':' kind ('=' expr)?

trait_list      ::= ':' trait_ref ('+' trait_ref)*
trait_ref       ::= type_expr

where_clause    ::= 'where' constraint_expr (',' constraint_expr)*

member          ::= param_decl | port_decl | sub_decl | derived_decl
                   | constraint_line | connect_stmt | chain_stmt
                   | entity_decl | field_body

param_decl      ::= 'param' IDENT ':' type_expr ('=' expr)?
port_decl       ::= 'port' IDENT ':' dir? type_expr ('{' member* '}')?
sub_decl        ::= 'sub' IDENT ':' type_expr ('{' member* '}')?
derived_decl    ::= 'derived' IDENT ':' type_expr '=' expr
constraint_line ::= 'constraint' (constraint_ref | expr)

dir             ::= 'in' | 'out'

connect_stmt    ::= 'connect' port_ref connect_op port_ref
                     (':' type_expr ('{' member* '}')?)? port_map?
connect_op      ::= '->' | '<->'
chain_stmt      ::= 'chain' IDENT ('->' IDENT)+

port_ref        ::= path ('@' selector ('(' args ')')? )?
port_map        ::= '{' (IDENT '->' IDENT)+ '}'

instance        ::= IDENT ':' type_expr ('{' member* '}')?

expr            ::= literal | IDENT | path | expr binop expr | unop expr
                   | expr '(' args ')' | lambda | conditional
                   | 'forall' IDENT 'in' expr ':' expr
                   | 'exists' IDENT 'in' expr ':' expr
                   | 'undef' | 'auto'

lambda          ::= '|' params '|' expr
conditional     ::= 'if' expr 'then' expr 'else' expr

literal         ::= INT | REAL | BOOL | STRING | quantity | range
                   | list_lit | set_lit | map_lit
quantity        ::= (INT | REAL) unit_expr
range           ::= expr '..' expr | expr '..<' expr
                   | cmp_op expr
set_lit         ::= 'set' '{' expr (',' expr)* '}'
```

---

## 12. Open questions and deferred decisions

### 12.1 Multi-line expressions
Long constraint expressions, field lambdas, and connection chains need line-continuation rules. Current plan: implicit continuation when a line ends with an operator, open bracket, or `->`. Explicit continuation with trailing `\` if needed. To be tested against real-world examples.

### 12.2 Pattern matching
Not included in this draft. May be needed for conditional structure definitions (`match cell_type { Gyroid => ..., Octet => ... }`). Deferred until use cases are clearer.

### 12.3 Error types and diagnostics
No `try`/`catch` or error types in this draft. Manufacturing feasibility violations, unsatisfiable constraints, and `auto` resolution failures are reported through the constraint/determinacy system, not exceptions. Whether a structured error/diagnostic type is needed at the language level (vs. purely toolchain-level) is an open question.

### 12.4 Literal syntax for geometric primitives
How to write point, vector, and orientation literals inline. Current strawman: `point(1mm, 2mm, 3mm)`, `vec(0, 0, 1)`, `orient(axis = z, angle = 45deg)`. These look like function calls but might warrant dedicated syntax if they're frequent enough. To be tested.

### 12.5 String-keyed metadata
Some engineering contexts need unstructured metadata (description fields, revision notes, vendor part numbers). Current approach: use `String` parameters or a `metadata` block. No special syntax proposed yet.

### 12.6 Syntax for `chain` with port mapping
The `chain` sugar assumes default ports. When processes have multiple inputs/outputs, does `chain` extend with explicit port mapping, or do you drop to explicit `connect`? Current leaning: drop to `connect` — `chain` is for the simple linear case.

---

*Document generated from syntax design sessions. Intended as a first draft to be refined through iteration.*
