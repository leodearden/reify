# Parameters

Parameters (`param`) are the public interface of a structure. They represent configurable values that exist on the determinacy spectrum.

## Syntax

```
param thickness : Length                        // No default, starts as undef
param width : Length = 50mm                     // Has default value
param material : M                              // Type parameter
param coating : Option<CoatingSpec> = none      // Optional parameter
param wall_thickness : Length = auto            // Solver decides
param wall_thickness : Length = auto(free)      // Free exploration mode
```

## Determinacy States

Parameters exist on a determinacy spectrum:
1. **`undef`** — not yet decided (no default, not specified)
2. **Default value** — has default, not overridden
3. **Determined** — explicitly set or computed to a concrete value
4. **`auto`** — delegated to the constraint solver
5. **Constrained** — bounded by constraints but not fully determined

## Rules

- Type annotation is mandatory
- Default value is optional (if absent, parameter is `undef`)
- Three-way distinction for unspecified parameters:
  1. No default, not specified → `undef`
  2. Has default, not specified → default value
  3. Explicitly `undef` → `undef` even if default exists

## Determinacy Predicates

```
determined(p)              // Has a concrete value
constrained(p)             // Has constraints but may not be fully determined
undetermined(p)            // No value, no constraints
partially_determined(p)    // Some but not all dimensions determined
```

## Setting Parameters

When instantiating a sub-structure:
```
sub bracket : Bracket {
    thickness = 3mm
    width = 100mm
}
```
