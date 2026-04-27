# integration_full_v01

[← Index](index.md)

## `pub structure Bolt` <a id="Bolt"></a>

*Optimized: `area`*

A standard fastening bolt.

### Parameters

| Name | Type | Dimension | Default | Description |
| --- | --- | --- | --- | --- |
| `length` | `Length` | — | `100 mm` | Bolt length. *hint: discrete_set(standard_bolt_lengths)* |
| `diameter` | `Length` | — | `M8` |  |

### Constraints

- `length >= diameter` *(line 42)*

### Meta

- **version**: 1.0

### Conforms to

- [`Physical`](#Physical)

