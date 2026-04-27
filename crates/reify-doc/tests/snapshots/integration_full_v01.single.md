# integration_full_v01

Comprehensive v0.1 language feature integration.

## Contents

### Traits

- [`Physical`](#Physical)

### Structures

- [`Board`](#Board)
- [`Bolt`](#Bolt)
- [`OldThing`](#OldThing)

### Occurrences

- [`MCU`](#MCU)

### Enums

- [`Grade`](#Grade)

### Functions

- [`safety_factor`](#safety_factor)

### Constants

- [`Positive`](#Positive)
- [`Pressure`](#Pressure)
- [`mil`](#mil)
- [`minimize_area`](#minimize_area)

## `pub type Pressure` <a id="Pressure"></a>

Pressure is Force per Area (SI unit: Pa).

= `Force / Area`

## `pub unit mil` <a id="mil"></a>

One mil = 1/1000 inch.

**Base:** `Length`

**Scale:** `0.0000254`

## `pub enum Grade` <a id="Grade"></a>

Material grade classification.

### Variants

- Standard
- Reinforced
- Premium

## `pub fn safety_factor` <a id="safety_factor"></a>

Safety factor for real-valued loads.

```reify
fn safety_factor(load: Real) -> Real
```

## `pub trait Physical` <a id="Physical"></a>

Trait for objects with a measurable mass.

### Members

- mass: Mass

## `pub constraint Positive` <a id="Positive"></a>

Length value v is strictly positive.

`v > 0mm`

## `purpose minimize_area` <a id="minimize_area"></a>

**Direction:** minimize

**Expression:** `total_area`

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

## `pub structure Board` <a id="Board"></a>

Main PCB board.

### Ports

| Name | Kind | Role | Type | Description |
| --- | --- | --- | --- | --- |
| `pwr_in` | — | in | `Power` | voltage, current |

## `pub occurrence MCU` <a id="MCU"></a>

Microcontroller occurrence.

### Used by

- [`Board`](#Board)

## `pub structure OldThing` <a id="OldThing"></a>

> **Deprecated:** use Bolt instead

Deprecated legacy structure.

## Tests

## `structure TestSelfWeight` <a id="TestSelfWeight"></a>

Self-weight regression test.

