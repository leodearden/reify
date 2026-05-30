# PRD: Unit Expressions in the Grammar

**Milestone:** version-agnostic language foundation (`docs/prds/unit-expressions.md`)
**Status:** active — authored 2026-05-26
**Type:** grammar/parser extension implementing language-spec §2.7 (unit expressions) + §5.1 (`^` operator), which are specified but unimplemented.

---

## 1. Goal (user-observable)

A `.ri` author can write compound and exponentiated units directly inside a quantity
literal, and can exponentiate scalars with `^`:

```reify
param density      : Density              = 7850kg/m^3
param gravity       : Acceleration         = 9.81m/s^2
param torque        : Torque               = 5kN*m
param area          : Area                 = 25mm^2
param viscosity     : DynamicViscosity     = 0.001kg/m/s     // (kg/m)/s — left-assoc
param conductivity  : ThermalConductivity  = 0.5W/(m*K)      // parenthesised group
param stress_sq     : Real                 = (5mm ^ 2) / (1mm ^ 2)   // value-level ^
```

Today **none of these parse** (verified 2026-05-26: `tree-sitter parse --quiet` exits 1
with `(ERROR ...)` nodes on each). The only working idiom is the compositional
workaround `7850.0 * 1kg / (1m * 1m * 1m)` (`examples/dimensional_chains.ri:84`,
`crates/reify-compiler/stdlib/materials_fea.ri:135`).

When this PRD lands, `reify check`/eval resolves each literal to the correct
dimensioned `Scalar` (SI value + `DimensionVector`), and the workaround disappears
from the standard library and examples.

## 2. Background

- The language spec already designs this surface:
  - **§2.7** — "Units compose with `*`, `/`, and `^` in postfix position after a
    number"; precedence "`^` binds tightest, then `*` and `/` left-to-right;
    parentheses available for disambiguation." Examples: `2.1kg/m^3`,
    `5kg*m/(s^2)`, `5(kg*m/s)^2`.
  - **§5.1** — `a ^ n` is a listed arithmetic operator; `Scalar<Q> ^ integer literal n
    → Scalar<Q^n>`; non-integer exponents on dimensioned quantities are type errors
    in v0.1.
- The implementation never caught up. Current grammar (`tree-sitter-reify/grammar.js`):
  - `quantity_literal` = `number_literal` + a single `immediate_identifier` aliased to
    `unit` (line 901–908) — one base unit, no operators.
  - `binary_expression` (line 846) has `* /` (prec 6) but **no `^`**.
  - `dimensional_type_expr` (line 707) has `* /` but no `^` (out of scope here — see §10
    and the companion stub PRD).
- Lowering (`crates/reify-syntax/src/ts_parser.rs:2686`) produces
  `ExprKind::QuantityLiteral { value: f64, unit: String }` — unit carried as a flat
  string and resolved late.
- Resolution: `crates/reify-compiler/src/units.rs` — `unit_to_scalar` (hardcoded base
  units) and `UnitRegistry` mapping `name → UnitEntry { dimension: DimensionVector,
  factor: f64, offset: Option<f64> }`. No compound-unit evaluator exists.
- The gap is documented as a "known limitation" in at least
  `docs/prds/v0_3/kernel-geometry-queries.md:29` and a comment block in
  `crates/reify-compiler/tests/materials_fea_tests.rs:271`.

## 3. Sketch of approach

Two grammatical surfaces, one shared dimension-algebra core:

1. **Unit expression inside a quantity literal** (`7850kg/m^3`). A structured
   `unit_expr` CST subtree, lexed contiguously (no internal whitespace) via an external
   scanner, lowered to a `UnitExpr` AST node, resolved against the `UnitRegistry`.
2. **Value-level `^`** (`5mm ^ 2`, `x ^ 3`). A new right-associative binary operator in
   `binary_expression`, typed per spec §5.1.

Both reduce to the same operation on `DimensionVector`s: exponent-vector addition
(`*`), subtraction (`/`), and integer scaling (`^`). The resolver and the type checker
share that algebra.

### 3.1 Tokenization — structured CST + external scanner (decision)

The unit expression is a real CST subtree, **not** an opaque token. An external scanner
(`tree-sitter-reify/src/scanner.c`, new) enforces the **contiguity invariant** so the
parser can give the unit expression internal structure (and support parentheses)
without losing disambiguation.

**Contiguity invariant.** A `unit_expr` begins only immediately after a
`number_literal` with **no intervening whitespace**, and **no whitespace may appear
anywhere inside it**. Whitespace (or any non-unit-expr character) terminates the unit
expression. This is load-bearing for disambiguation:

| Source | Parse |
|---|---|
| `5kg*m` | one `quantity_literal`, unit = `kg*m` (Torque) |
| `5kg * m` | `binary_expression`: `quantity_literal(5kg)` `*` `identifier(m)` |
| `5(kg*m/s)^2` | one `quantity_literal`, unit = `(kg*m/s)^2` |
| `5 * (kg*m/s)^2` | `binary_expression` (and `m`,`s` are unresolved identifiers → diagnostic) |

The scanner's job is precisely to emit the unit-expression token boundary only when the
next character is adjacent, and to suppress the normal `extras` whitespace-skipping
within the unit-expression region. The implementing task owns the exact scanner
mechanics; the acceptance signal is the fixture table in §7.

### 3.2 Grammar (unit_expr)

```
unit_expr   := unit_pow
             | unit_expr ('*' | '/') unit_pow        // left-assoc, prec 1
unit_pow    := unit_atom ('^' integer)?              // prec 2 (tighter)
unit_atom   := unit_name | '(' unit_expr ')'
unit_name   := /[A-Za-z_][A-Za-z0-9_]*/
integer     := '-'? /[0-9]+/                          // signed
```

- `^` binds tighter than `*`/`/`; `*`/`/` are left-associative (so `kg/m/s` =
  `(kg/m)/s` = kg·m⁻¹·s⁻¹, the dynamic-viscosity case).
- Exponents are signed integers (`s^-2` is legal; equivalent to `/s^2`).
- Parentheses group sub-expressions and may be raised to a power (`(kg*m/s)^2`).

### 3.3 Value-level `^`

Added to `binary_expression`:
- Precedence **above** multiplicative and unary (so `-2^2 = -(2^2)`, `2*3^2 = 2*9`).
- **Right-associative** (`2^3^2 = 2^(3^2)`).
- Grammar accepts `_expression '^' _expression`; the **integer-literal-exponent
  restriction for dimensioned bases is enforced in the type checker**, not the grammar
  (so the diagnostic is a clear type error, not a parse error).

## 4. Interface contract (cross-crate; the H component)

The `UnitExpr` AST and the `^` typing rules cross crate boundaries
(`reify-syntax` lowering ↔ `reify-compiler` resolution/typing ↔ `reify-eval`). These
are fixed here so the tasks below cannot drift.

### 4.1 UnitExpr AST (`reify-types`)

```rust
pub enum UnitExpr {
    Unit(String),                       // base/derived unit name, e.g. "kg"
    Mul(Box<UnitExpr>, Box<UnitExpr>),
    Div(Box<UnitExpr>, Box<UnitExpr>),
    Pow(Box<UnitExpr>, i32),            // signed integer exponent
}
// QuantityLiteral.unit changes:  String  ->  UnitExpr
// A bare `5mm` lowers to QuantityLiteral { value: 5.0, unit: UnitExpr::Unit("mm") }.
```

### 4.2 Resolver (`reify-compiler`, `units.rs`)

```rust
/// Fold a unit expression against the registry into an SI conversion factor and
/// a net dimension vector.  si_value = numeric_value * factor.
pub fn resolve_unit_expr(
    expr: &UnitExpr,
    registry: &UnitRegistry,
) -> Result<(f64 /* factor */, DimensionVector), UnitResolveError>;

pub enum UnitResolveError {
    UnknownUnit { name: String, span: SourceSpan },
    AffineUnitInCompound { name: String, span: SourceSpan }, // see 4.4
    // (room for future: ZeroToNegativePower, etc.)
}
```

Folding rules:
- `Unit(n)` → registry lookup; `(entry.factor, entry.dimension)`.
- `Mul(a,b)` → `(fa*fb, da + db)` (dimension vectors add).
- `Div(a,b)` → `(fa/fb, da - db)`.
- `Pow(a,n)` → `(fa.powi(n), da * n)` (dimension vector scaled by n).

### 4.3 `^` typing table (value-level; from spec §5.1)

| Base | Exponent | Result |
|---|---|---|
| `Int` | `Int` (≥ 0) | `Int` |
| `Real` (`Scalar<Dimensionless>`) | `Real` | `Real` |
| `Scalar<Q>` | **integer literal** `n` | `Scalar<Q^n>` |

Non-integer exponent on a dimensioned base → type error
(`E_NONINT_EXP_ON_DIMENSIONED` or nearest existing code; use `sqrt` for half-integer).
Negative integer `n` on `Scalar<Q>` is legal (`Q^-n`).

### 4.4 Affine units in compound expressions

Affine units (`UnitEntry.offset.is_some()`, e.g. `degC`, `degF`) have no meaningful
multiplicative composition. `5degC/m` is rejected with `AffineUnitInCompound`. A bare
affine literal (`20degC`) continues to work through the existing standalone path. State
this as a diagnostic, not a silent mis-resolution.

## 5. Resolved design decisions

1. **Structured CST via external scanner**, not an opaque token — parentheses are
   supported in v1 because disallowing them is surprising for readability (`W/(m*K)`).
2. **Value-level `^` is in scope** (§5.1) alongside unit-literal `^`; they share the
   dimension-exponent algebra.
3. **Signed integer exponents** everywhere (`s^-2`, `m^3`). Non-integer exponents on
   dimensioned values are type errors (spec §5.1).
4. **`/` is left-associative** → `kg/m/s` means kg·m⁻¹·s⁻¹; multi-factor denominators
   need no parentheses, but parentheses are available.
5. **Affine units rejected in compound expressions** (§4.4).
6. **Type-level `^` (`type Area = L^2`) is out of scope** — companion stub PRD
   `docs/prds/dimensional-type-exponent.md`.
7. **`unit: String` → `unit: UnitExpr`** AST change; lowering and resolver rewritten to
   the structured form (§4.1).

## 6. Pre-conditions for activating

None external — this PRD *is* the grammar work it depends on. The grammar/scanner task
(α) and the value-level-`^` grammar task (δ) are the G3 grammar prerequisites, tracked
as first-class tasks with parse-fixture observable signals (§7). No upstream PRD gates
this.

## 7. Acceptance fixtures (parser-side + resolver-side)

**Parser-side** (task α / δ — `tree-sitter parse --quiet` exits 0; CST shape asserted in
`tree-sitter-reify/tests/`):

| Fixture | Must parse as |
|---|---|
| `7850kg/m^3` | quantity_literal, unit = Div(kg, Pow(m,3)) |
| `9.81m/s^2` | quantity_literal, unit = Div(m, Pow(s,2)) |
| `5kN*m` | quantity_literal, unit = Mul(kN, m) |
| `25mm^2` | quantity_literal, unit = Pow(mm,2) |
| `0.001kg/m/s` | quantity_literal, unit = Div(Div(kg,m),s) |
| `0.5W/(m*K)` | quantity_literal, unit = Div(W, Mul(m,K)) |
| `5(kg*m/s)^2` | quantity_literal, unit = Pow(Mul(kg,Div(m,s))…)^2 |
| `5mm ^ 2` | binary_expression, op `^`, right-assoc |
| `5kg * m` (neg. test) | binary_expression — NOT one literal |
| `5 kg/m^3` (neg. test) | NOT a quantity_literal (space after number) |

**Resolver-side** (task γ — dimension + SI assertions):

| Input | Asserts |
|---|---|
| `7850kg/m^3` | `Scalar { si_value ≈ 7850.0, dim = MASS·LENGTH⁻³ }` |
| `9.81m/s^2` | dim = LENGTH·TIME⁻² (Acceleration) |
| `5kN*m` | `si_value ≈ 5000.0`, dim = Torque |
| `0.001kg/m/s` | dim = MASS·LENGTH⁻¹·TIME⁻¹ (DynamicViscosity) |
| `5kgg/m` (err) | `UnknownUnit { name: "kgg" }` with span |
| `5degC/m` (err) | `AffineUnitInCompound { name: "degC" }` |

## 8. Decomposition plan (bare-B vertical slices + integration gate)

Labels are PRD-local; task IDs assigned at decompose time.

| α | **Grammar + external scanner: unit expressions.** | Crates: `tree-sitter-reify`. | *Signal:* the parser-side fixture table in §7 — `tree-sitter parse --quiet` exits 0 on each positive case and the two negative cases parse as binary/non-literal; `tree-sitter-reify/tests/` asserts the CST shape. | Prereqs: none. *Intermediate* (unlocks β, δ). |
| β | **AST + lowering: `UnitExpr`.** | Crates: `reify-types`, `reify-syntax`. | *Signal:* lowering unit test round-trips each §7 positive fixture to the expected `UnitExpr` tree; bare `5mm` still lowers to `Unit("mm")`. | Prereqs: α. *Intermediate* (unlocks γ). |
| γ | **Resolver: evaluate `UnitExpr` against `UnitRegistry`.** | Crates: `reify-compiler`. | *Signal:* the resolver-side fixture table in §7 (SI value + dimension assertions + the two error cases with spans). | Prereqs: β. *Intermediate* (unlocks ε). |
| δ | **Value-level `^` operator.** | Crates: `tree-sitter-reify`, `reify-syntax`, `reify-compiler` (typing), `reify-eval`. | *Signal:* `5mm ^ 2` evaluates to a `Scalar` area (dim = LENGTH²); `2.0 ^ 3.0` = `8.0` Real; `5mm ^ 1.5` is a type error (`E_NONINT_EXP_ON_DIMENSIONED`); each pinned in a test. | Prereqs: α (shares `^` token / grammar regen). *Intermediate* (unlocks ε). |
| ε | **Integration gate (leaf).** | Crates: `reify-eval` (e2e), `examples/`. | *Signal:* `examples/unit_expressions.ri` declares density, acceleration, torque, area, viscosity, conductivity, and a value-level `5mm^2`; a `crates/reify-eval/tests/unit_expressions_e2e.rs` evaluates the file and asserts each resolves to the correct SI `Scalar`. `reify check examples/unit_expressions.ri` is clean. | Prereqs: γ, δ. **Leaf** — the user-observable signal. |
| ζ | **Tidy-up: migrate workaround sites to the new idiom.** | Crates: `reify-compiler/stdlib`, `examples/`. | *Signal:* the ~9 inventoried lib/example sites (§ below) are rewritten to compound literals; the workspace test suite stays green (values unchanged); a grep for the compositional density idiom (`1m \* 1m \* 1m`) in `stdlib/` and `examples/` returns only files with a stated reason to keep it. **Equivalence tests asserting `mm^2 == mm*mm` are kept** (they have a reason to exist). | Prereqs: ε. **Leaf**. |
| η | **Companion prose corrections.** | Crates/docs: PRDs + test comments. | *Signal:* the "known grammar limitation" note at `docs/prds/v0_3/kernel-geometry-queries.md:29`, the comment at `crates/reify-compiler/tests/materials_fea_tests.rs:271`, and the `money-dimension.md` workaround references are updated to state compound literals now parse (grep confirms the old text is gone). | Prereqs: ε. **Leaf**. |

Dependency edges: α→β→γ; α→δ; γ→ε; δ→ε; ε→ζ; ε→η.

**Tidy-up inventory (for ζ, verified 2026-05-26):**
`crates/reify-compiler/stdlib/materials_fea.ri:135,173,211,252` (4 densities),
`crates/reify-compiler/stdlib/materials_electrical.ri:62,76` (resistivity bounds),
`crates/reify-compiler/stdlib/structural_physical.ri`,
`examples/drivebelt_trait_bounds.ri:111`,
`examples/topology_selectors/all_topology_selectors_wiring.ri:44`.

**KEEP (not migratable) — `std.units` registry-bootstrap constraint:**
`STANDARD_GRAVITY`, `SPEED_OF_LIGHT`, and `BOLTZMANN_CONSTANT` in
`crates/reify-compiler/stdlib/units.ri` stay in their compositional
`<n> * 1m / (1s * 1s)` form. Compound-unit literals resolve via
`resolve_unit_expr`, which requires a seeded unit registry in scope; but
`std.units` is the module that *builds* that registry, so its own function
bodies compile in the registry-less bootstrap scope guarded in `expr.rs`
(the `None` branch). Param-default scopes in later-compiled modules
(materials_electrical, structural_physical) DO seed the registry, which is
why those ζ sites migrated cleanly. STANDARD_GRAVITY was originally listed
above as migratable; corrected to KEEP per esc-3809-87 (2026-05-30). Each
constant carries a keep-reason comment at its definition site so future
readers / task η do not retry. Unblocking this would require `expr.rs` to
forward the in-construction registry into `std.units` function-body
compilation — out of scope for this PRD.

**KEEP (not migratable) — no-prelude example-harness compile context:**
`examples/integration_corner_cases.ri` (jerk), `examples/dimensional_chains.ri`
(n_unit/rho/mu_dyn/p_area/g_earth), `examples/math_linalg.ri` (n_unit), and
`examples/dimensional_consistency.ri` (n_unit/g_accel) stay in their
compositional bare-literal form. These four fixtures are compiled by their
test harnesses (`integration_corner_cases.rs`, `stress_dimensional_chains.rs`,
`m8_stdlib_integration.rs`) through the **no-prelude** `reify_compiler::compile`
entry point, which has no unit registry in scope (see `lib.rs` doc-comment).
Bare unit literals (`1kg`, `1mm`, `1s`) resolve via the standalone fallback, but
compound-unit literals route through `resolve_unit_expr` against the empty
registry and fail with "unknown unit" — even base units like `m`/`s`/`kg` are
absent, so there is no value-preserving compound form here. Originally listed
above as migratable; corrected to KEEP per esc-3809-89 (2026-05-30) — same
root-cause class as the std.units constraint. Each fixture carries a
keep-reason comment. Migrating these would mean switching the harnesses to
`compile_with_stdlib`, which risks prelude collisions with the fixtures'
locally-defined aliases (`type Velocity`, `let n_unit`) and expands scope to
two harness files outside this task — out of scope. The example sites that DO
compile with the stdlib prelude (`drivebelt_trait_bounds.ri`,
`all_topology_selectors_wiring.ri`) remain in the migrate inventory above.

## 9. Cross-PRD relationship

No contested-ownership seam — this PRD owns the grammar, scanner, lowering, and
resolution end to end. It is a pure *producer* unblocking downstream consumers:

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_3/kernel-geometry-queries.md` | consumes | density arg as compound literal (`7850kg/m^3`) | this PRD | corrected in η |
| `docs/prds/money-dimension.md` | consumes | `25USD/kg` cost-per-mass literal | this PRD | corrected in η |
| `docs/prds/dimensional-type-exponent.md` (stub) | sibling | shares dimension-exponent algebra; type-level `^` | that PRD | filed as stub |
| stdlib materials / examples | consumes | the new idiom | this PRD (ζ) | migrated in ζ |

## 10. Out of scope

- **Type-level `^`** in `dimensional_type_expr` (`type Area = L^2`, `Scalar<L^2>`) — its
  own gap, companion stub `docs/prds/dimensional-type-exponent.md`.
- **Per-module unit-registry plumbing into post-passes** (the pragma-resolution
  limitation noted in memory; separate concern).
- **New units** — this PRD composes existing registry entries; it does not add base or
  derived units.
- **Non-integer / symbolic exponents** on dimensioned quantities (spec keeps these type
  errors in v0.1).

## 11. Open questions (tactical)

1. **Scanner whitespace-suppression mechanism.** Whether the external scanner tokenizes
   the whole unit-expr region itself or emits a boundary token that disables `extras`
   for the region. **Suggested resolution:** implementer's choice provided the §7
   contiguity fixtures (positive + the two negative cases) pass. Decide during task α.
2. **Diagnostic code reuse for non-integer exponent.** Whether to reuse an existing
   `E_*` code or mint `E_NONINT_EXP_ON_DIMENSIONED`. **Suggested resolution:** reuse the
   nearest existing dimensional-type-error code if one fits; otherwise mint. Decide
   during task δ.
3. **`Pow` exponent storage width.** `i32` is proposed; confirm no realistic unit needs
   beyond ±2³¹. **Suggested resolution:** `i32`. Decide during task β.
