# Collection Types

## Types

| Type | Purpose |
|------|---------|
| `List<T>` | Ordered sequence (bolt patterns, point clouds) |
| `Set<T>` | Unordered unique collection (material options) |
| `Map<K, V>` | Key-value mapping (property tables) |
| `Range<T>` | Bounded interval — `2mm..5mm` |
| `Option<T>` | Explicit optionality — `some(v)` or `none` |

## Option vs Undef

`Option` is a type-level statement about existence (may or may not be present). `undef` is a determinacy state (value not decided yet). A parameter is always present — it may just be `undef`.

## Operations

- **List:** `count`, `sum`, `map`, `filter`, `fold`, `all`, `any`, `contains`, `[i]`, `generate(n, fn)`, `concat`
- **Set:** `count`, `contains`, `union`, `intersection`, `difference`
- **Map:** `[key]` lookup, `keys`, `values`, `count`, `contains_key`
- **Range:** `contains`, `lower`, `upper`, `span`

## Literals

```
[1, 2, 3]                           // List
set{a, b, c}                        // Set (prefix avoids block ambiguity)
map{"key" => value, "k2" => v2}     // Map
```

## Empty Collections

- `[].sum` requires element type context
- `forall x in []: P(x)` → `true` (vacuous truth)
- `exists x in []: P(x)` → `false`

## Counted Sub-structures

```
sub vents : List<Vent>
constraint vents.count == vent_count
```

The runtime recognizes `count == N` constraints on `List<Structure>` as structure-controlling, triggering schema re-elaboration.
