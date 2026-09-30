# PRD (forward-stub): generic assemblies & interface traits

**Milestone:** v0_6 · **Status:** DEFERRED forward-stub · **Date:** 2026-09-25
**Tracker:** task #7882 (`[MILESTONE]` pure gate; fires when its preconditions land).
**Provenance:** esc-7809-5 design session with Leo (`design-7809-trait-ports`, 2026-09-25).
Seeded by task #7809, which found that spec §4.2 lists "Ports" as a trait member kind that
neither the grammar nor the compiler implements.

**Scope ruled by Leo:** the expansion covers the **full programme**, not trait-required
ports alone:

- **A.** trait-required ports;
- **B.** generic assemblies (generic / trait-typed `sub` slots, with member and port access
  through the bound);
- **C.** generic chains.

## Why deferred

Trait-required ports (`trait Actuator { port output : out MotivePort }`) are sound and fit the
language's direction. The spec's own generics example is
`FlexibleCoupling<DriverPort: RotaryPort, …>`, and assembly-modal-connection-graph §6 decision 4
rules that "ports are the attachment invariant". On today's substrate, though, the feature on
its own delivers only a declaration-site conformance check that nothing downstream reads. Its
compelling uses (B and C below) are each blocked by other missing features. Designing
covariance, direction and compound-port rules with no consumer to validate them would fail
/prd G1.

In the meantime:

- #7809 makes a `port` line in a trait body a loud, located parse error. Today it is silently
  dropped, and so is any garbage in a trait body.
- Spec §4.2 marks the row deferred and points here.

## Substrate gap (verified 2026-09-25)

Probed with `target/debug/reify` (built 2026-09-22; the relevant compiler files are unchanged on
main `746f417893`).

| Probe | Result | Owner |
|---|---|---|
| `trait T { port p : out X }` + a conformer without `p` | `All constraints satisfied.` The requirement is dropped: `trait_member` (tree-sitter-reify/grammar.js) has no port arm, `lower_trait_members` swallows the ERROR, and `compile_trait` skips via `_ =>` | #7809 (loud error) → this PRD |
| `structure def G<T: HasShaft> { sub m : T }` | `error: sub-component "m" references unknown structure "T"` (entity.rs lowers `MemberDecl::Sub` to `Type::StructureRef(name)`) | **this PRD (B)** |
| `sub m : SomeTrait` (trait-typed sub) | `unknown structure "SomeTrait"` | **this PRD (B)** |
| `structure def G<T: HasShaft> { param m : T }`, never instantiated | debug `check` **panics**: `unrepresentable cell_type … TypeParam("T")` | #7225 |
| `sub b = Bearing<GasketSeal>()` (explicit type arg) | never monomorphised; `T`-typed cells degrade to Undef | #6853 |
| `param m : ElasticMaterial`; `m.youngs_modulus` | typed `Real`, not `Pressure` | #6870 |
| `connect motor.nonexistent -> coupler.bore` (concrete sub) | `OK connect_compat_…` | #7880 |
| `connect oth (out Other) -> bore (in Shaftish)` | `OK connect_compat_…` (port-type compatibility is never checked) | #7881 |
| `connect a.b.p -> …` (nested path) | `invalid port reference in connect statement` | #7162 |
| `let d = motor.shaft.diameter` | `member access not yet supported: .diameter` (port members are not value cells) | #5995 family |
| `chain a -> b` over `param a : A` | `undefined port 'a' in connect statement` (chain cannot range over param slots) | **this PRD (C)** |
| `MotorMount<auto: NemaMotor>()` filtering on a trait param | selects correctly (`MotorMount$Nema23`); reads through the slot afterwards are INDETERMINATE | this PRD (B) |

## Sketch (when activated)

**A. Interface conformance** — the one piece that is useful on its own:

```
trait Actuator {
    param rated_power : Power
    port power  : in PowerPort
    port output : out MotivePort
}
structure def HalfActuator : Actuator { param rated_power : Power = 50W }
// want: error: missing required port 'output' (out MotivePort)
```

Today's workarounds are both unsound:

- **Duck-typed trait constraint.** `constraint determined(shaft.diameter)` only reports
  `unresolved name: shaft`.
- **Mirrored `param`.** The trait requires `param shaft_diameter`, and each conformer wires its
  port to it by hand. A conformer can lie: param 5mm, port 12mm, and the check still says `OK`.

**B. Generic assemblies** — the headline:

```
trait NemaMotor { param body_width : Length   port shaft : out Shaftish }
structure def MotorMount<M: NemaMotor> {
    sub motor : M
    sub coupler = Coupler()
    connect motor.shaft -> coupler.bore
    constraint motor.shaft.diameter == coupler.bore.diameter
}
```

**C. Generic chains.** §6.2 inference is fed from the bound's required `in`/`out` ports:

```
trait ProcessStep { param duration : Time   port stock : in Work   port part : out Work }
occurrence def Line<A: ProcessStep, B: ProcessStep> { sub a : A   sub b : B   chain a -> b }
```

## Open design questions (rule at expansion)

1. **Port-type matching.** Trait params use exact type match (§4.2, v0.1). A port requirement
   naturally wants refinement-compatible narrowing: a `RotaryPort` port satisfying a
   `MotivePort` requirement. Should ports diverge from the param rule?
2. **Direction.** Does a `bidi` port satisfy an `out` requirement? Direction is spelled two ways
   today: the `in`/`out` keyword and the `Port.direction` param (default `Bidi`). Which one
   does a requirement check?
3. **Compound ports vs structure interfaces.** What does a `port` member inside a trait that
   *refines `Port`* mean? That would be a compound port with sub-ports. §6.1.2's NEMA17 mapping
   example (`shaft -> input_bore`, `bolt_hole_1 -> mounting_a`) reads like one, but
   `connect.rs::auto_match_port_members` implements §6.1.2 as port-*member* matching. Reject it,
   or design compound ports deliberately.
4. **Erasure vs monomorphisation for generic subs.** `auto` type params monomorphise today
   (`MotorMount$Nema23`); generic fns and enums are type-erased (§3.9.1/§3.9.2).
   - If generic subs are erased, trait-required ports are *mandatory*: they are the only way to
     type `motor.shaft`.
   - If generic subs are monomorphised, trait-required ports are early checking only.
5. **Default ports in traits.** Recommended out for v1. `Sub` requirements have no default
   concept either (`DefaultKind` has no Sub/Port arm), so default ports would be new machinery
   (L).
6. **Access through a bound.** `member_access_on_type_param` (crates/reify-compiler/src/expr.rs)
   resolves only `param`/`let` requirements (`CompiledTrait::value_bearing_members`) to a flat
   `ValueRef`. A port is a namespace plus a connect endpoint, not a scalar, so it needs a new
   code path.

## Cost evidence

**A alone** (conformance only, required ports, no defaults):

- **Grammar:** one `trait_member` arm in tree-sitter plus a corpus file. There is no new
  conflict: `[$.port_declaration]` already covers it.
- **Already done:** the lezer grammar (`TraitDeclaration` reuses `Block`), `ts_parser`
  (`lower_trait_members` → `lower_member` already has a port arm) and the AST
  (`MemberDecl::Port` / `PortDecl`).
- **New work:**
  - `compile_trait` port arm, replacing the silent `_ =>` skip;
  - `RequirementKind::Port { direction, port_type }` plus 6 exhaustive matches;
  - merge/dedup and a conflict diagnostic;
  - conformance check;
  - doc-build and test-support.
- **Size:** about 150–200 LOC of production code, or 500–800 with tests — one task. That is
  roughly 0.5–1× task 3972 ιᵦ (15 files, +1120 LOC).
- **Runtime:** none. The work is purely compile-time.

**Precedents** (per-task isolated diffs; generated tree-sitter files are untracked):

| Chain | Tasks | LOC | Calendar |
|---|---|---|---|
| trait-assoc-fn (3934–3957) | 8 | +7835/−348, 2807 of it tests | 28 days; ~3.5 weeks of that was infra stalls |
| trait-assoc-type (3971–3974) | 4 | +3541/−63 | 14 days |

The fn chain still owes follow-ups #7866 and #7856, 3–4 months after landing.

**B + C** add generic/trait-typed sub slots (no task today, sized L), access through the bound,
connect and chain through generic subs, and port-member value cells. This is unmeasured; judged
at least assoc-fn-chain scale.

## Consumer

No consumer exists in the tree today: `prj/printer_v01/printer.ri` declares zero ports and
zero connects. Candidates to confirm at expansion:

- the toolchanger's tool/dock interface, where every tool presents the same kinematic-coupling
  and electrical ports;
- reusable motor mounts over NEMA frames;
- process lines over interchangeable steps.

G1 must name a real one before decomposition.

## Pre-conditions for activating

These are task #7882's dependency edges: #6853, #7225, #6870, #7162, #7880, #7881.
Related, not gating: #7809, #7856, #7866, #7659, #7374, #5995, #7387, #5024.

## Relationship to other PRDs

- **`assembly-modal-connection-graph.md` / `flexible-assembly-modal.md`:** enabled, not
  constrained. Ports stay the single attachment vocabulary (§6 decisions 2 and 4). The
  per-port requirement those PRDs need lives on `LocatedPort` / `RegionPort`, not on a body
  trait.
- **`placement-relations-belt.md` §7.3:** orthogonal. `connect` between `LocatedPort` pairs
  lowers to a `relate` fasten mate.
- **`structural-query-traversal.md`:** a different generic-over-trait idiom (find-by-trait),
  not an interface contract.

No conflict was found with any planned programme.

## Out of scope

- Purpose `.ports` reflection: separate gap; purposes are per-structure by design.
- Default ports in traits: see question 5.
- Generic-trait type-args: #5024.

## Decomposition (when activated — not filed now)

- **α** — trait-required ports: grammar arm, `RequirementKind::Port`, conformance (A).
- **β** — generic/trait-typed `sub` slots: type resolution, sub-component validation, and
  monomorphisation or erasure per question 4.
- **γ** — member and port access through the bound: `value_bearing_members`,
  `member_access_on_type_param`.
- **δ** — connect/chain through generic subs, with direction and type checks fed from the
  bound's required ports.
- **ε** — vertical slice on the named consumer.
