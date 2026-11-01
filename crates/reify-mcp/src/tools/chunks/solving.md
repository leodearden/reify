# Letting the solver choose a value (`auto`)

To let the solver pick a dimension instead of writing a number, give it the value `auto` and bound it with constraints. The solver chooses a value that satisfies every constraint, and every resolving command reports with that value in place.

```reify
structure def Plate {
    param thickness : Length = auto
    constraint thickness >= 2mm && thickness <= 4mm
}
```

`reify check` and `reify eval` on that structure print:

```text
  OK Plate#constraint[0]
All constraints satisfied.

Plate.thickness = 0.003 m
```

With no objective, a value bounded on both sides resolves to the centre of its bracket: 3mm here.

## Where `auto` may appear

`auto` is valid only at a binding site, where a named slot directly receives its value:

```reify-fragment
param thickness : Length = auto
let gap : Length = auto
sub x : Leaf { length = auto }
sub x = Leaf(length: auto)
connect a -> b { gain = auto }
```

These are a `param` default, a `let` binding, a sub-instance override, a constructor argument, and a connect parameter. `auto` is not an expression operand: it cannot appear inside arithmetic, as a function argument, or in a `constraint`, `minimize` or `maximize` body. Write the bound as a separate constraint instead.

A constructor argument set to `auto` is solved from the PARENT's constraints:

```reify
structure def Leaf {
    param length : Length = 5mm
    let doubled = length * 2
}

structure def Assembly {
    sub x = Leaf(length: auto)
    constraint self.x.length >= 8mm && self.x.length <= 12mm
}
```

`reify eval` prints `Assembly.x.length = 0.01 m`, the centre of the bracket. Read the sub's members through `self`, as in `self.x.length`. A bare `x.length` in the parent's constraint is not resolved: the constraint is reported INDETERMINATE (`operator undefined for these operand kinds: StructureInstance`), the `auto` warns W_UNDERDETERMINED, and plain `reify check` still exits 0.

## Which commands resolve `auto`

These resolve every `auto` before they report: `reify check` (including `--strict`), `reify test`, `reify build`, `reify eval` (and its alias `reify run`), `reify report`, `reify explain`, and the GUI viewport.

Not yet: the editor (language-server diagnostics and hover) and the GUI's definition-preview pane. They show such a value as `undef`, and a constraint over it as INDETERMINATE, even when `reify check` resolves it.

## Bound it from both sides, or give it an objective

A strict `auto` (the default) must be determined. Bounded on one side only, it is an error, exit 1, naming the missing bound. For `param thickness : Length = auto` with only `constraint thickness >= 2mm`:

```text
error: strict auto parameter resolution is not uniquely determined: no constraint bounds `Plate.thickness` above. Only the constraints declared in `Plate` take part in its solve, and they bound it only below, so a solve could only return the edge of the solver's internal default domain — add the missing bound in `Plate`, give `Plate` an objective, or declare it `auto(free)`
```

Of the remedies that message lists, add the missing bound. An objective does not stand in for it today. With only `constraint thickness >= 2mm`, `minimize thickness` still makes `reify check` exit 1, with the shorter `error: strict auto parameter resolution is not uniquely determined`, and `maximize thickness` returns `Plate.thickness = 10 m`, the edge of the solver's internal domain, with exit 0 and no warning.

Add the other bound and the value resolves to the bracket centre. An objective whose optimum lies strictly inside the bracket moves the value to that optimum; this one resolves `thickness` to about 3mm, and `reify eval` prints `Shim.thickness = 0.003000000000000555 m`:

```reify
structure def Shim {
    param thickness : Length = auto
    constraint thickness >= 2mm && thickness <= 6mm
    minimize (thickness - 3mm) * (thickness - 3mm)
}
```

An objective that pushes the value toward one of its bounds does not reach that bound today. What happens depends on the direction of the push. Over the same [2mm, 6mm] bracket:

- **Toward the lower bound** (`minimize thickness`; `maximize -thickness` likewise): the value is 4mm, the bracket centre. `reify check` exits 0 with no warning.
- **Toward the upper bound** (`maximize thickness`; `minimize -thickness` likewise): `reify check` exits 1 with `error: strict auto parameter resolution is not uniquely determined`. The constraint reads INDETERMINATE and the value stays `undef`. `reify eval` exits 1 too.

Switching to free mode, as that error suggests, does not reach the bound either: over the same bracket it returns 4mm under either `minimize thickness` or `maximize thickness`, with the free-mode warning. If you need the value at a bound, write that bound as the constraint that fixes it.

Free mode is an explicit opt-in for exploration. It returns some feasible value and warns that the value is not uniquely determined:

```reify-fragment
param thickness : Length = auto(free)
```

## When it does not resolve

- **No feasible value.** Contradictory bounds, such as `thickness >= 4mm && thickness <= 2mm`, make `reify check` fail, exit 1, with `error: proven infeasible: no solution exists in the searched domain`.
- **No constraint touches it.** An `auto` that no constraint reads warns W_UNDERDETERMINED and stays `undef`. `reify eval` explains it as `awaiting solve`. Because no constraint reads it, nothing goes INDETERMINATE, so even `reify check --strict` exits 0. Read the warnings.
- **A constraint cannot be decided.** `reify check --strict` fails, exit 1, on any INDETERMINATE constraint. Plain `reify check` reports it and exits 0.

## Sub-instance caveat

A constructor-argument or override `auto` is solved for the parent's constraints, but the sub's own derived members and geometry are not re-evaluated at the solved value. In the `Assembly` example above, `reify eval` prints (excerpt):

```text
Assembly.x.doubled = undef
Assembly.x.length = 0.01 m
```

So do not rely on a sub's lets, or its geometry, following a solved constructor argument yet. A child whose own definition declares an `auto` is not resolved per instance either.

Related: constraint syntax and objectives are topic `constraints` of `reify_language_reference`; the determinacy predicates are topic `parameters`.
