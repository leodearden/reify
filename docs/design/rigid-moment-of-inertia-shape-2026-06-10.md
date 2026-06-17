# Rigid.moment_of_inertia: scalar vs `Tensor<2,3,MomentOfInertia>` — recommendation memo

**Date:** 2026-06-10
**Scope:** design/analysis deep-dive for deferred task **4229** ("Auto-derive `Rigid.moment_of_inertia` from geometry"), precondition (b): the scalar-vs-tensor shape decision.
**Method:** static analysis with file:line evidence + empirical probes run against a fresh `target/debug/reify` (2026-06-09 build; the `target/release/reify` binary is stale — it predates the task-α dimension tightening and must not be used for probing). Probe files preserved at `/tmp/moi-probe{1..8}.ri`.
**No stdlib or production code was changed.**

## 1. The question

Task 4229's end goal is to convert `Rigid.moment_of_inertia` from a user `param` to a geometry-derived
`let moment_of_inertia = moment_of_inertia(geometry, material.density)`. The builtin returns
`Tensor<2,3,MomentOfInertia>` (`crates/reify-compiler/src/units.rs:159`, `:246-252`; eval dispatch
`crates/reify-eval/src/geometry_ops.rs:2617`, `:3649`, `dispatch_inertia_tensor` ~`:4563`), while the trait
member is a scalar (`crates/reify-compiler/stdlib/structural_physical.ri:58`) with a scalar-shaped
constraint (`:59`). Should the member be widened to the full inertia tensor, and what are the consequences?

## 2. Premise corrections (empirically verified — these change the option analysis)

Three premises in the task/brief framing turned out to be wrong or incomplete:

1. **"A `> 0` comparison does not type-check against a rank-2 tensor" — false.** The type checker has
   **no operand guard on relational ops**: `infer_binop_type` returns `Type::Bool` unconditionally for
   `Eq/Ne/Lt/Le/Gt/Ge` (`crates/reify-compiler/src/type_compat.rs:857-872`; contrast Add/Sub dimension
   checks at `crates/reify-compiler/src/expr.rs:1111-1149`). At runtime `eval_cmp` falls through to
   `as_f64()`, which is `None` for `Value::Tensor`, so the comparison yields `Value::Undef`
   (`crates/reify-expr/src/lib.rs:3635-3668`; `crates/reify-ir/src/value.rs:1121`). **Probe:** a
   `constraint <defined tensor> > 0.0*1kg*1m*1m` compiles with zero diagnostics and reports
   `INDETERMINATE … undefined inputs` from `reify check` — permanently, even when the tensor is fully
   defined. So widening does not *break* the constraint loudly; it silently neuters it. That is worse
   than a type error and must be handled deliberately (§4).

2. **"The only live blocker is the shape decision" — false; there is a third blocker.** The
   `moment_of_inertia` builtin's density argument is resolved by `resolve_density_arg`
   (`crates/reify-eval/src/geometry_ops.rs:3961-3987` → `resolve_real_scalar_arg` `:3996-4012`), which
   accepts **only a `ValueRef` to a bare `Value::Real` or dimensionless scalar**:
   - `material.density` is dimensioned `Density` (kg/m³, tightened by task #3111) → `Severity::Warning`
     + `Undef`. **Probe 5:** `let d = material.density; let i = moment_of_inertia(b, d)` → warning
     "density argument must be a bare numeric Real … in v0.3" and `i = undef`.
   - An **inline literal** `moment_of_inertia(b, 7850.0)` is a non-`ValueRef` expr → **silent** `Undef`,
     no diagnostic at all (probes 1, 4). Only the let-bound bare-Real form works
     (`examples/kernel_queries/moment_of_inertia_box.ri:27-31`, and its grammar note `:16-19` documents
     this v0.3 restriction).

   Therefore the 4229 target expression `moment_of_inertia(geometry, material.density)` **cannot produce
   a value today regardless of the shape decision** — it compiles cleanly and evals `Undef` (probe 2).
   This is precisely the G6 field-population failure 4229's gate exists to prevent. Precondition (a)
   (kernel seam, task 4237) **is** wired — `crates/reify-eval/src/dynamics_ops.rs:228-258` issues real
   `Volume`/`CenterOfMass`/`InertiaTensor` queries — but that wiring serves `body_mass_props`, whose
   density ladder (`resolve_body_density`, `dynamics_ops.rs:269-272, 330`) *does* handle
   `Material.density`. The `moment_of_inertia` topology-selector path has no such ladder. Call this
   **precondition (c)**; it needs its own small task (§6).

3. **"Keep scalar needs a (currently-absent?) tensor-reduction op" — reductions already exist.**
   `trace`, `determinant`, `eigenvalues`, `complex_eigenvalues`, `inverse`, `transpose` are DSL-exposed
   (`crates/reify-compiler/src/math_signatures.rs:47-79`) and **dimension-preserving**: `trace` →
   `Scalar<Q>` (`:265`), `eigenvalues` → `List<Scalar<Q>>` (`:273`). **Probe 5:** on a defined
   `Tensor<2,3,MomentOfInertia>`, `trace` → `0.093 m²·kg` and `eigenvalues` → `[0.013, 0.04, 0.04] m²·kg`
   (sorted ascending by the runtime, `crates/reify-stdlib/src/matrix.rs:~240`). No new op is required by
   any option.

   Bonus: because eigenvalues come back sorted, an exact **positive-definiteness constraint is
   expressible today**: **probe 6** — `let eigs = eigenvalues(itens)` +
   `constraint eigs[0] > 0.0*1kg*1m*1m` reports `OK` for a PD tensor and `VIOLATED` for an indefinite
   one (negative middle eigenvalue). List indexing in constraints works.

(Minor citation fix: the builtin's eval-side registration lines `2617`/`3649` are in
`crates/reify-eval/src/geometry_ops.rs`, not reify-compiler.)

## 3. Findings per investigation point

### 3.1 `Rigid` conformers (blast radius)

Conformers of the **stdlib** `Rigid` (all currently redeclare the scalar param, as required for a
no-default trait param — conformance machinery: `crates/reify-compiler/src/conformance/checker.rs:1320-1557`):

| Conformer | Where | Current member |
|---|---|---|
| `RigidPost` | `examples/structural_traits_dimensioned.ri:17-24` | `param … : MomentOfInertia = 0.04 * 1kg*1m*1m` (geometry: 100×100×300 mm box, steel) |
| `BoltFlange` | `examples/m5_geometry_flange.ri:3-25` | `param … : MomentOfInertia = 0.002 * 1kg*1m*1m` (geometry: CSG flange) |
| test fixture | `crates/reify-compiler/tests/structural_physical_tests.rs:1111` | scalar param |
| test fixture | `crates/reify-compiler/tests/material_struct_tests.rs:265` | scalar param |
| LSP probe fixtures | `crates/reify-lsp/src/analysis.rs:753-756`, `crates/reify-lsp/src/diagnostics.rs:403-406` | scalar param |

Shape-pinning tests (not conformers, but assert the scalar shape and would need updating under any
widening): `structural_physical_tests.rs:417-469` (`rigid_refines_physical_with_moment_of_inertia`,
asserts `Param` kind + `Scalar{MOMENT_OF_INERTIA}`) and `:1334-1335` (member dimension).

**Out of blast radius** (they define a *local* `trait Rigid` that shadows the stdlib name):
`examples/m5_trait_rigid.ri:1-10`, `crates/reify-compiler/tests/trait_bounds_tests.rs:94-97, 192-196`
(and siblings), `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:1518-1520`.

**Break-loudness, by trait-member shape (probes 7, 8):**
- *Required tensor param (no default):* stale scalar conformer → **loud error**
  `type mismatch for trait member 'moment_of_inertia': expected Tensor2x3<Scalar[m^2·kg]>, got Scalar[m^2·kg]`.
- *Tensor param with derived default:* stale scalar redeclaration is **silently accepted** — the
  redeclaration type check does not fire when the trait param has a default (probe 7). This looks like a
  conformance-checker gap worth filing independently of 4229.
- *`let` (the 4229 endgame):* a conformer param **silently shadows** the trait let — trait let defaults
  are injected only when the structure doesn't declare the name (`conformance/checker.rs:1561-1844`).
  No error; the auto-derive is inert for that conformer.

**Precedent (`Physical.mass`):** the trait-`let` shape is already canonical for derived members —
`let mass = volume(geometry) * material.density` (`structural_physical.ri:46`) is auto-injected, no
conformer redeclares it, and it evaluates to a real value through a `param geometry : Solid` slot
(verified: `RigidPost.mass = 23.55 kg`). Note the asymmetry that hides blocker (c): `mass` multiplies a
kernel `volume()` result by `material.density` in ordinary scalar arithmetic, whereas
`moment_of_inertia` needs density *inside* the kernel query and so hits `resolve_density_arg`.

Parent-trait params (`geometry`, `material`) **are in scope** for a refining-trait `let` —
param defaults are registered before let compilation (`conformance/checker.rs:501-554`, `:612`), and
probe 2 verified the exact target form compiles, including the let member sharing the builtin's name
(no shadowing problem: cell names and function-call names live in different namespaces).

### 3.2 The `> 0` constraint (`structural_physical.ri:59`)

Covered by §2.1/§2.3: under widening, the constraint as written compiles silently and is permanently
INDETERMINATE. Trait constraints are injected per-structure and compiled against the conformer's actual
member (`conformance/checker.rs:1846-1872`, `trait_requirements.rs:271-327`), so during any transition
window the same trait constraint can be a working scalar compare for one conformer and indeterminate for
another. Replacement options, all verified working today:

- **Exact positive-definiteness** (recommended): `let moi_eigs = eigenvalues(moment_of_inertia)` +
  `constraint moi_eigs[0] > 0.0 * 1kg * 1m * 1m` — probe 6 confirms both OK and VIOLATED paths.
  PD is the right v1 invariant; it matches what the Rust-side hook enforces for `MassProperties`
  (PSD via `dynamics_psd.rs` — `dynamics.ri:44-47`). (The full physical-realizability triangle
  inequalities on principal moments could be added later; nothing else in the codebase enforces them.)
- **Trace positivity** (weaker): `constraint trace(moment_of_inertia) > 0.0 * 1kg * 1m * 1m` — works
  (probe 5) but admits indefinite tensors.

### 3.3 Dynamics consumers

**There are zero production readers of `Rigid.moment_of_inertia`.** Full-repo sweep: every
`moment_of_inertia` hit in `crates/` is the builtin's own machinery, tests, or fixtures; no solver,
eval, or GUI path reads the trait member. It is a declare-only contract today.

The actual rigid-body-dynamics stack already standardized on the **full 3×3 tensor**, via a parallel
surface that bypasses `Rigid` entirely:
- `MassProperties.inertia : Matrix<3,3,Real>` (`crates/reify-compiler/stdlib/dynamics.ri:71-83`),
  populated from geometry by `body_mass_props` through the task-4237 kernel seam
  (`dynamics_ops.rs:228-258`, `:284-392`).
- RNEA `inverse_dynamics` / `inverse_dynamics_at_snapshot` (`dynamics.ri:237-252`) extract
  `(mass, com, inertia)` as a 3×3 (`crates/reify-stdlib/src/dynamics/eval.rs:314-330`, used at `:730-781`).

So: **no consumer wants the scalar; the only inertia consumers in the codebase want the tensor.**
A scalar member's implied "principal/uniaxial reduction" serves nobody, and there is no axis convention
anywhere to make a scalar well-defined. (Side observation: `MassProperties.inertia` is dimensionally
untyped `Real`; a `Tensor<2,3,MomentOfInertia>` Rigid member would be *better*-typed than the RBD
surface itself, and could anchor a future `MassProperties` tightening.)

### 3.4 Scalar reduction (if scalar were kept)

The ops exist (§2.3), so option B costs no new machinery — but it forces a semantic choice with no
principled answer: `trace/3`? the max principal moment (`eigs[2]`)? a named-axis diagonal (no axis
convention exists on `Rigid`)? Each is wrong for some consumer, and the member's meaning becomes
"some scalar, axis unspecified" — exactly the latent physical wrongness the brief flags. Verdict:
feasible, cheap, and physically arbitrary.

### 3.5 The param→let auto-derive conversion (concrete sketch)

```reify
trait Rigid : Physical {
    let moment_of_inertia = moment_of_inertia(geometry, material.density)
    let moi_principal = eigenvalues(moment_of_inertia)
    constraint moi_principal[0] > 0.0 * 1kg * 1m * 1m
}
```

- **Compiles today** (probe 2: identical form on a local trait — clean compile, derived member appears
  in a non-declaring conformer). `geometry`/`material` resolve from `Physical` (§3.1).
- **Evaluates `Undef` until precondition (c) lands** (§2.2). With (c) fixed, the box probes show the
  kernel path produces analytically-correct tensors (probe 5 `ParamGeomLetDensity`: I_xx=I_yy=0.19625,
  I_zz=0.03925 kg·m² for the 100×100×300 mm steel box — matches m(a²+b²)/12 by hand).
- **Degradation (G6):** a conformer whose geometry doesn't realize gets `Undef` → the PD constraint
  reports INDETERMINATE (warning, not error) — the same degradation contract as `Physical.mass` and the
  defensive-Undef path in `dynamics_ops.rs:369-383`. Acceptable and consistent.
- **Migration:** the two example conformers' scalar params would silently shadow the let (§3.1), so the
  conversion must delete/replace them in the same change: `RigidPost` drops its param (becoming the
  showcase for auto-derive); `BoltFlange` likewise (its hand-estimated 0.002 kg·m² becomes a derived
  tensor). Fixtures at `structural_physical_tests.rs:1111`, `material_struct_tests.rs:265`, and the two
  LSP probe fixtures need the param dropped or converted to a `matrix(...)` tensor override (probe 3
  confirms `param itens : Tensor<2,3,MomentOfInertia> = matrix([[…]])` works end-to-end with
  dimensioned elements — there is no tensor *literal* syntax, but the `matrix()` builtin fills that role).
  Shape-pinning tests `structural_physical_tests.rs:417-469` and `:1334-1335` re-pin to Let + tensor.

## 4. Option analysis

| | (A) widen to tensor (as param→let auto-derive) | (B) keep scalar + reduction | (C) carry both |
|---|---|---|---|
| Physical correctness | **Correct** — inertia of a general body is rank-2 | Wrong for general bodies; axis assumption undeclared | Correct tensor + redundant scalar |
| Matches builtin / RBD stack | **Yes** — no adapter anywhere | Needs a reduction at the derive site | Tensor leg yes |
| Conformer blast radius | 2 examples + 4 test fixtures + 2 shape tests; **zero consumers** | none | same as A |
| Constraint | Replace with eigenvalue-PD — **works today** (probe 6) | keep `> 0` | both |
| New ops needed | **None** | None (reductions exist) — but an arbitrary semantic choice | None |
| Trait-idiom alignment | **Matches `Physical.mass`** derived-let precedent | Keeps the one member that contradicts the derive idiom | No precedent for paired members |
| Hidden cost | Silent shadow by stale conformer params (mitigated: migrate the only two in-repo; optional lint later) | Member stays consumer-less and physically underspecified | Extra surface with no named consumer for the scalar leg (G1-style orphan) |

The hybrid considered along the way — tensor **param with derived default** (override-able without
shadow semantics) — is rejected for now: probe 7 shows conformer redeclarations against a
defaulted trait param are **not type-checked** (a stale scalar override is silently accepted), so it
offers no safety over the let while diverging from the `Physical.mass` idiom. The underlying checker gap
is worth filing regardless.

## 5. Recommendation

**Option A — widen to `Tensor<2,3,MomentOfInertia>`, implemented directly as 4229's param→let
auto-derive, with the eigenvalue positive-definiteness constraint.** Do not pass through an intermediate
"tensor param" state; land the let form in one change with conformer/fixture migration (§3.5).

Rationale in one line each:
- The member has **zero consumers**, so the usual reason to keep a scalar (consumer churn) does not exist; the entire cost is 6 fixture edits.
- Every actual inertia consumer in the codebase (RNEA stack) already uses the full 3×3; the builtin already returns it; a scalar would need an axis convention nobody has defined.
- All allegedly-missing machinery already exists and is verified working: tensor surface type, `matrix()` construction, dimension-preserving `trace`/`eigenvalues`, list-indexed PD constraint with real violation detection.
- It matches the `Physical.mass` derived-let idiom — the PRD's own original "auto-computed" intent (docs/prds/v0_6/structural-traits-reconciliation.md §5 δ).
- Scalar conveniences need no member: any consumer can write `eigenvalues(moment_of_inertia)[0]` / `trace(...)` at the use site, which is why (C) is unnecessary surface.

## 6. Follow-ups to unblock 4229

1. **New prerequisite task — precondition (c): density-arg resolution.** Extend
   `resolve_density_arg` (`crates/reify-eval/src/geometry_ops.rs:3961-3987`) to accept
   (i) a `ValueRef` to a `Density`-dimensioned `Scalar` (si_value is already SI kg/m³ — near-trivial),
   and ideally (ii) inline-literal and field-access arg shapes (currently *silent* Undef — at minimum
   they should warn). Mirror `resolve_body_density`'s ladder (`dynamics_ops.rs`) or reuse it. Also
   update the stale grammar note in `examples/kernel_queries/moment_of_inertia_box.ri:16-19` and the
   smoke fixture if the v0.3 restriction is lifted. Wire as a dependency of 4229.
2. **4229 implementation (after Leo ratifies the shape):** trait edit per §3.5 sketch; migrate
   `RigidPost`, `BoltFlange`; update `structural_physical_tests.rs:417-469`, `:1097-1157`, `:1334-1335`,
   `material_struct_tests.rs:265`, `reify-lsp/src/analysis.rs:753-756`, `reify-lsp/src/diagnostics.rs:403-406`;
   add an e2e asserting a non-declaring `Rigid` conformer evals a non-Undef, analytically-correct tensor
   (the task's `user_observable_signal`).
3. **Optional hygiene tasks surfaced by this investigation:**
   - Conformance checker: type-check conformer redeclarations against defaulted trait params (probe 7 gap).
   - Type checker: warn (or error) on relational ops over tensor operands — today `tensor > scalar`
     compiles silently and is permanently indeterminate (§2.1), a foot-gun independent of 4229.

Per the deferred-vs-pending norm, 4229 itself stays `deferred` until Leo ratifies the shape decision;
once ratified, file (c), wire the dependency, and flip 4229 to `pending`.
