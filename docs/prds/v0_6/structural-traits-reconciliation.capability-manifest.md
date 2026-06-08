# Capability manifest — `structural-traits-reconciliation`

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/structural-traits-reconciliation.md`.
Evidence forms per the Reify overlay (`.claude/skills/prd/project.md` → "Capability Manifest").
Empty-value sentinel: `Value::Undef`. All bindings checked 2026-06-02.

---

## α — Tighten `structural_physical.ri` §4 dimensioned params  (LEAF, active)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `MomentOfInertia` resolves as `.ri` type alias | capability→producer | `grep:crates/reify-compiler/src/type_resolution.rs:1962` `resolve_type_name("MomentOfInertia")`; registered `crates/reify-types/src/dimension.rs:362-393` | **PASS** (wired on main) |
| `Pressure` resolves as `.ri` type alias | capability→producer | `grep:crates/reify-compiler/stdlib/constitutive.ri:92` `param e1 : Pressure`; `grep:crates/reify-compiler/stdlib/materials_fea.ri:89` `youngs_modulus : Pressure` (live conformers on main) | **PASS** |
| `Length` resolves as `.ri` type alias | capability→producer | `grep:crates/reify-compiler/stdlib/ports_mechanical.ri:67` `param thread_diameter : Length` | **PASS** |
| `Temperature` resolves as `.ri` type alias | capability→producer | registered `dimension.rs:362-393`; placeholder-audit table-D classifies `max_service_temp` tightenable-now | **PASS** |
| Tightened trait bodies + dimensioned constraint RHS parse | grammar-fixture | `grammar-fixture:/tmp/prd-gate-fixtures/structural-tighten.ri` → `tree-sitter parse --quiet` exit 0, 0 ERROR; unit literals `1Pa`, `1kg*1m*1m`, `1K`, `1N/1m` each parse | **PASS** |
| `reify check` rejects wrong-dimension assignment (negative fixture) | end-to-end (G6 branch 3) | dimensional checker rejects dim-mismatch in conformance (pattern proven by existing `Pressure` conformers); **positive** `reify check` pass is the load-bearing signal — exact `E_*` code confirmed at impl | **PASS** (positive case); diagnostic-code tactical |
| Field-population (result-field twin) | n/a | α is a compile-time type-tightening; it does **not** sample a result field | **N/A** |

**No FAIL bindings.** α is queueable.

---

## β — Reconcile `docs/reify-stdlib-reference.md` §4  (LEAF, active; depends on α)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| §4 doc matches shipped+tightened `structural_physical.ri` | textual consistency | doc deliverable; no runtime capability asserted; verified by diffing §4 code block against the .ri member set after α | **N/A** (documentation) |

No runtime capability asserted; no FAIL possible.

---

## γ — (DEFERRED) Flexible continuum stiffness-tensor field

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| A consumer samples a body-level `Field<Point3,Tensor<2,3,Pressure>>` stiffness | capability→producer (G1) | none on main — FEA reads moduli from `material : MaterialSpec` slot | **FAIL** (`producer-absent`) → **this is why γ is parked, not active** |
| `Field<…>`/`Tensor<2,3,Pressure>` type | grammar+type | parses + resolves (`type_resolution.rs:1419`) | PASS (substrate OK; consumer is the blocker) |

Parked `deferred` until a named consumer exists. The orphan binding is the gate working as intended.

---

## δ — (DEFERRED) Auto-derive Rigid.moment_of_inertia from geometry

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `moment_of_inertia(Solid, Density)` produces a non-`Undef` value | field-population / end-to-end | `grep:crates/reify-eval/src/dynamics_ops.rs:223,287` — kernel seam **unwired**, returns `Undef` (task 3620 TODO) | **FAIL** (`producer-absent` / `Undef`) → **this is why δ is parked, not active** |
| scalar vs `Tensor<2,3,MomentOfInertia>` shape reconciled | design | builtin returns a Tensor; trait member is scalar | unresolved (deferred) |

Parked `deferred` until task 3620 wires the kernel seam and the shape is reconciled.
