# Deferred Syntax Completion: Design Decisions

**Status:** Complete — resolves all remaining deferred syntax items from `deferred-syntax-items-design-decisions.md` §10: port mapping syntax, multi-line continuation rules, metadata blocks, and the `require` vs `constraint` question.  
**Version:** 0.1 — First crystallisation from deferred syntax completion session  
**Builds on:** `deferred-syntax-items-design-decisions.md` v0.1, `syntax-design-decisions.md` v0.1, `module-system-design-decisions.md` v0.1, `constraint-system-design-decisions.md` v0.1

---

## 1. Design approach

The deferred syntax items document (v0.1) left four items explicitly deferred. This phase resolves all four, guided by the same principles: regularity, concision, explicitness, readability, and parseability.

The central finding is that **no new language-level concepts are introduced**. Port mapping uses existing syntax forms in a `connect` block. Multi-line continuation follows simple, predictable rules. Metadata uses a new block keyword but no new semantics. The `require` keyword is dropped in favour of determinacy predicates composed through the existing `constraint` mechanism.

---

## 2. Port mapping syntax

### 2.1 Same-interface automatic matching

When both sides of a `connect` statement implement the same interface trait, ports are matched by name automatically. No mapping block is needed:

```
connect motor.nema17 -> mount_plate.nema17 : NEMA17BoltSet { grade = 8.8 }
```

### 2.2 Explicit mapping for different interfaces

When the two sides implement different interfaces (or the same interface but a non-default mapping is desired), a complete explicit mapping block is required:

```
connect motor.nema17 -> adapter.side_a {
    shaft -> input_bore
    bolt_hole_1 -> mounting_a
    bolt_hole_2 -> mounting_b
}
```

Every port on both sides must appear in the mapping. Partial mappings are not permitted.

**Rationale:** All-or-nothing mapping eliminates the case where two ports coincidentally share a name across unrelated interfaces and are silently matched. If both sides share an interface trait, the name matching is intentional (the trait defines the contract). If they don't, every correspondence must be stated explicitly.

### 2.3 Explicit mapping overrides automatic matching

If a mapping block is provided on a connection where automatic matching would otherwise apply, the mapping block takes precedence and automatic matching is fully suppressed. This handles the case where you want a non-default mapping between two ports of the same type (e.g., swapping two symmetric ports):

```
// Both sides implement the same interface, but we swap two ports
connect panel_a.connector -> panel_b.connector {
    left -> right
    right -> left
    ground -> ground
}
```

The rule is: if you provide a mapping block, you own the entire mapping.

### 2.4 Mixing port mappings and connector parameters

A `connect` block may contain both port mappings (`->`) and connector parameter assignments (`=`). The parser distinguishes them by operator:

```
connect motor.nema17 -> adapter.side_a : AdapterPlate {
    thickness = 5mm          // Connector parameter (= operator)
    shaft -> input_bore      // Port mapping (-> operator)
    bolt_hole_1 -> mounting_a
    bolt_hole_2 -> mounting_b
}
```

Port mappings with `->` are syntactic sugar — they desugar to constraints on the connector structure binding its ports to the specified targets.

### 2.5 `chain` remains simple

`chain` uses fully implicit matching via default ports only. If non-default port mapping is needed at any step, use explicit `connect` statements. `chain` is for the simple linear case:

```
chain casting -> machining -> heat_treat -> finishing
```

**Rationale:** Extending `chain` with inline port designations would produce something that looks like a sequence of `connect` statements but with less clarity. The purpose of `chain` is concision for the common case; complexity belongs in `connect`.

---

## 3. Multi-line continuation rules

### 3.1 Design principle

Reify is newline-significant: newlines terminate declarations and statements within `{ }` blocks. Continuation rules specify when a newline does *not* terminate.

### 3.2 Continuation inside delimiters

Inside `()` and `[]`, newlines are whitespace. Free continuation with no special rules:

```
let moment = (
    force * lever_arm
    + secondary_force * secondary_arm
    - damping_coefficient * angular_velocity
)

fn cross_product(
    a : Vector3<Length>,
    b : Vector3<Length>
) -> Vector3<Area> {
    vec3(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x
    )
}
```

This is the primary mechanism for multi-line expressions. When in doubt, wrap in parentheses.

### 3.3 Inside `{}`

Inside `{ }` blocks (entity bodies, connector blocks, mapping blocks), newlines separate declarations and statements. Each line is a member declaration, a predicate expression, or a parameter assignment.

### 3.4 Trailing continuation

A line ending with any of the following tokens continues to the next line:

- Binary operators: `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `>=`, `<=`, `>`, `<`, `&&`, `||`, `and`, `or`, `implies`
- Connection operators: `->`, `<->`
- Comma: `,`
- Opening delimiters: `(`, `[`, `{`

```
connect motor.shaft ->
    coupling.driver

constraint rated_torque >=
    load_torque * safety_factor

chain casting -> machining ->
    heat_treat -> finishing
```

### 3.5 Leading operators do not continue

A line *starting* with a binary operator does not implicitly continue the previous line. To use leading-operator style for readability, wrap the expression in parentheses:

```
// Does NOT work — two separate (broken) expressions:
constraint rated_torque
    >= load_torque * safety_factor

// Works — parentheses enable free continuation:
constraint (rated_torque
    >= load_torque * safety_factor)
```

**Rationale:** Trailing-only continuation is simpler to specify and parse (LL(1)-friendly). Parenthesised wrapping handles all leading-operator cases cleanly. Leading-operator continuation can be added later as a purely additive change if experience shows the parenthesis requirement is burdensome.

### 3.6 No backslash continuation

There is no `\` line continuation character. Parenthesised wrapping is always available and more readable.

**Rationale:** Backslash continuation is a wart in every language that has it. The combination of free continuation inside `()` / `[]` and trailing-operator continuation covers all practical cases without it.

---

## 4. `require` vs `constraint`

### 4.1 Decision: `require` is dropped

The `require` keyword used in purpose definitions in the constraint system and module system documents is replaced by `constraint` with determinacy predicates.

```
// Before (constraint-system-design-decisions.md §7.2):
purpose def manufacturing_ready {
    require all geometric_params determined
    require all material_params determined
    minimize cost
}

// After:
purpose def manufacturing_ready {
    constraint forall p in geometric_params: determined(p)
    constraint forall p in material_params: determined(p)
    minimize cost
}
```

### 4.2 Determinacy predicates

Determinacy state is queried through boolean predicate functions:

- `determined(param)` — the parameter has a specific value
- `constrained(param)` — the parameter has at least one constraint applied
- `undetermined(param)` — the parameter is `undef` with no constraints

These are ordinary boolean predicates that happen to operate on determinacy state rather than values. They compose with `forall`, `exists`, `and`, `or`, and participate in `where` guards, exactly like value-level predicates.

### 4.3 Rationale

The categorical distinction between value-level predicates and determinacy-level predicates is real but not worth a dedicated keyword. Users — particularly those with a mechanical engineering background — should not need to understand a meta-level boundary to write purpose definitions. `constraint determined(geometric_params)` reads naturally as "this thing must be determined" without requiring awareness of the abstraction layer being crossed.

This is consistent with the project-wide principle of unification over special cases. `determined()` is a predicate; the constraint system handles predicates. No new keyword, no new concept, no decision about "is this a `require` or a `constraint`."

### 4.4 Keyword changes

- `require` is removed from the keyword set.
- `determined`, `constrained`, and `undetermined` are standard library functions, not keywords.

---

## 5. Metadata blocks

### 5.1 The `meta` block

A `meta` block provides string-keyed, string-valued metadata for unstructured engineering information that does not participate in the constraint system or evaluation graph:

```
structure def Bracket : RigidMechanical {
    meta {
        description = "L-shaped mounting bracket for sensor array"
        part_number = "BRK-2024-001"
        revision = "C"
        compliance = "ISO 9001"
    }

    param thickness : Length
    param width : Length = 50mm
}
```

### 5.2 Semantics

- Keys are identifiers (same lexical rules as other identifiers).
- Values are string literals.
- No types, no determinacy tracking, no constraint participation.
- Purely informational — metadata is opaque to the evaluation graph.

### 5.3 Scope

`meta` blocks can appear in any entity body (structures, occurrences, constraints, fields) and at the module level:

```
module acme.brackets

meta {
    author = "J. Smith"
    created = "2025-03-15"
}

structure def Bracket : RigidMechanical {
    meta {
        part_number = "BRK-2024-001"
    }
    // ...
}
```

### 5.4 Constraints on `meta` blocks

- **One per scope.** At most one `meta` block per entity body or module. Duplicate `meta` blocks are a compile error.
- **No duplicate keys.** Each key must be unique within a `meta` block. Duplicate keys are a compile error.
- **Not inherited.** Metadata does not propagate through traits or specialisation. A trait's metadata is the trait's metadata; an implementing structure has its own.

### 5.5 Access syntax

Metadata is accessed via dot notation through the `meta` namespace:

```
let pn = bracket.meta.part_number
```

The `meta` namespace is distinct from params, ports, and sub-structures, so no collision risk.

### 5.6 Rationale

Metadata is not parametric data. Description strings, part numbers, and revision codes don't sit on the determinacy spectrum, are never `auto`, and are never constrained. Representing them as `String` params pollutes the parameter namespace and misrepresents what they are. A dedicated `meta` block honestly represents metadata as opaque labels for human and toolchain consumption.

The data model is deliberately simple: string keys and string values. JSON or structured metadata would introduce a foreign syntax and data model inside Reify for marginal benefit. If structured metadata proves necessary, the `meta` block can evolve.

### 5.7 Doc comments vs `meta`

`///` doc comments and `meta` blocks coexist and serve different purposes:

- **Doc comments** (`///`): for tooling-generated documentation — API docs, hover text, inline help. Audience: developers using the definition.
- **Metadata** (`meta`): for engineering data that travels with the design — part numbers, revision codes, compliance references. Audience: engineers, PLM systems, manufacturing toolchains.

Different audiences, different lifecycles, different access patterns.

---

## 6. Syntax updates to prior documents

### 6.1 Grammar changes

The semi-formal grammar from `deferred-syntax-items-design-decisions.md` §9.1 is updated:

```
member          ::= param_decl | port_decl | sub_decl | let_decl
                  | constraint_line | connect_stmt | chain_stmt
                  | entity_decl | field_body | fn_decl
                  | where_block | match_block | meta_block

connect_stmt    ::= 'connect' port_ref connect_op port_ref
                     (':' type_expr)? connect_block?
connect_block   ::= '{' (param_assign | port_mapping)* '}'
param_assign    ::= IDENT '=' expr
port_mapping    ::= IDENT '->' IDENT

meta_block      ::= 'meta' '{' meta_entry* '}'
meta_entry      ::= IDENT '=' STRING_LIT
```

### 6.2 Keyword changes

- `require` is removed from the keyword set.
- `meta` is added.

### 6.3 Updates to prior documents

- **`constraint-system-design-decisions.md` §7.2**: `require` in purpose definitions is replaced by `constraint` with determinacy predicates (`determined()`, `constrained()`, `undetermined()`).
- **`module-system-design-decisions.md` §4.4**: `require` in the `Motor` example is replaced by `constraint determined(...)`.
- **`deferred-syntax-items-design-decisions.md` §10**: all four open questions (port mapping syntax, multi-line continuation rules, metadata blocks, `require` vs `constraint`) are resolved by this document.
- **`syntax-design-decisions.md` §6.5**: port mapping syntax is formalised. §12.1: multi-line continuation rules are resolved. §12.5: metadata syntax is resolved. §12.6: `chain` with port mapping is resolved (drop to `connect`).

---

## 7. Summary of key decisions

| Decision | Choice | Rationale |
|---|---|---|
| Same-interface port matching | Automatic by name | Interface trait defines the contract; matching is intentional |
| Different-interface port mapping | Complete explicit mapping required | All-or-nothing eliminates coincidental name collisions |
| Explicit mapping override | Mapping block suppresses automatic matching entirely | If you provide a mapping, you own the entire mapping |
| Mixed connect blocks | `->` for port mappings, `=` for params, same block | Unambiguous parse; no lookahead needed |
| `chain` port mapping | Default ports only; drop to `connect` otherwise | `chain` is for concision in the simple linear case |
| Newline significance | Newlines terminate declarations in `{ }` blocks | Consistent with no-semicolon style across all examples |
| Continuation inside `()` `[]` | Newlines are whitespace | Free continuation; primary mechanism for multi-line expressions |
| Trailing continuation | Line ending with operator, `->`, `,`, or opening delimiter continues | Covers connection chains, long constraints, parameter lists |
| Leading operator continuation | Not supported; use parentheses | Trailing-only is simpler to specify and parse; additive change later if needed |
| Backslash continuation | Not included | Parenthesised wrapping is always available and more readable |
| `require` keyword | Dropped; replaced by `constraint` with determinacy predicates | Unification over special cases; users need not understand the meta-level boundary |
| Determinacy predicates | `determined()`, `constrained()`, `undetermined()` as library functions | Boolean predicates over determinacy state; compose with existing constraint mechanisms |
| Metadata | `meta` block with string keys and string values | Honest representation; doesn't pollute params; no foreign syntax |
| `meta` block count | One per scope; duplicate keys are compile errors | Metadata is findable and unambiguous |
| `meta` inheritance | Not inherited through traits or specialisation | Avoids provenance confusion |
| `meta` access | Dot notation via `meta` namespace | Distinct from params/ports/subs; no collision |
| Doc comments vs `meta` | Coexist; different purposes | Doc comments for API docs; `meta` for engineering data |

---

*Document generated from deferred syntax completion session. Resolves all remaining open questions from `deferred-syntax-items-design-decisions.md` §10.*
