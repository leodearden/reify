# Where Guards

`where` controls structural presence — when the guard is false, the entity does not exist in the evaluation graph.

## Per-Declaration Guard

```
sub fan_mount : FanMount where needs_cooling { ... }
constraint vent_count >= 2 where needs_cooling
```

Rule: `where` comes after the "what" and before the body (if any).

## Block-Level Where

```
where needs_cooling {
    constraint vent_count >= 2
    sub fan_mount : FanMount { ... }
    sub vents : List<Vent> { ... }
}
```

Desugars to per-declaration guards. `where` blocks do NOT introduce a new lexical scope.

## Else Clause

```
where needs_cooling {
    sub fan_mount : FanMount { ... }
} else {
    sub passive_vents : List<PassiveVent> { ... }
}
```

Members in `else` block desugar to `where !condition` guards.

## Nesting

Guards compose conjunctively:
```
where is_structural {
    where needs_reinforcement {
        sub ribs : List<Rib> { ... }
        // Effective guard: is_structural and needs_reinforcement
    }
}
```

## Guard with Undef

When the guard expression is `undef`, the guarded entity is provisionally absent (not an error). As the guard's inputs become determined, the entity appears or disappears.
