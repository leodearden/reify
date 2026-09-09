# Design invariants

A gate checklist, not an essay. This is reify's adoption of the
dark-factory `docs/legibility/design-invariants.md` convention: `/prd`
decompose's G7 gate and `/review` phase 2 Read this path at run time and
walk every task/finding against each invariant's checkable question. It is
the single normative copy — do not restate invariant text elsewhere; cite
slugs. Stable slug ids are load-bearing (G7 waivers reference them).
Numeric aliases INV-SF-* are prose convenience only.

This first population is the **silent-failure family** (INV-SF-1..7),
from the 2026-07-24 silent-undef/placeholder eradication investigation
(dogfood session a0d342d4 → investigation session, probe evidence in the
task census: #5360, #5345, #5385, #5386, #5197, #5390, #5392). INV-SF-7
was appended 2026-07-25 from the same investigation's functional-
enumeration probe; its evidence (#5392, and #5492 as suspected drift of
the same seam) is recorded in its own section. Kleene 3-valued
logic and undef-as-value are deliberate design; these invariants target
**silence**, not undef. Conservative degradation (Indeterminate, skip,
refuse) stays correct — but must always leave an observable trace.

A second family, the **angle-crossing family** (INV-AD-1..4), was
appended 2026-08-10 from the angle-dimension-completion chartering
investigation (PRD `docs/prds/v0_6/angle-dimension-completion.md`,
decision D6). A third family, the **declared-surface family**
(INV-PD-1..2), plus the umbrella principle below, was appended
2026-08-31 from the trampoline param-drop and result-field vacancy
censuses (PRDs `docs/prds/v0_6/trampoline-param-drop-closure.md` and
`docs/prds/v0_6/result-field-vacuity-closure.md`). Other invariant
families may be appended later; keep slugs stable.

## The umbrella principle — `nothing-vacuous-and-unowned`

> Nothing should exist vacuously and unowned. Ever. Anywhere.
> — Leo, ratified 2026-08-31.

**Vacuous = declared-but-inert** — no machine reads or dispatches on
it. NOT empty: `NoDamping {}` / `FlexureJoint {}` are empty but
load-bearing (nominal type-tag dispatch, the ratified marker idiom of
`placeholder-type-eradication-ratchet.md`); the pre-closure `Part` was
empty AND inert. The banned state is the **conjunction** vacuous ∧
unowned — vacuous-with-a-live-owner is legitimate staged work, and IS
an allowlist entry in whichever gate covers the surface.

**Three exits per finding, no fourth state:** wire it / delete it /
allowlist-with-live-owner.

**Checkable design question(s)**: Does this feature declare anything —
a field, param, type, knob, hook, config key — that no machine reads
or dispatches on, and if so, which live task owns wiring or deleting
it? Does it add a new *declarable surface* on which vacuity can
accumulate — and if so, which detector gates it, or is its census row
below added marked convention-only?

**The census obligation.** Enforcement must be visible, never
presumed: every declarable surface is either detector-gated or
honestly marked convention-only. Unenforced invariants decay in this
repo (measured twice at the infra layer: the rerere re-armer, the
hooksPath clobber — see `CLAUDE.md` "Warm lanes"). The Status column
is mutable by design as detectors land; each owning PRD's close leaf
refreshes its row. **Adding a detector or a new declarable surface
requires a census row in the same change.**

| Surface | Gate | Status |
|---|---|---|
| trampoline-consumed `structure_def` params | PDROP (`trampoline-param-drop-closure.md`) | chartered |
| result fields + engine-attached undeclared fields | PVAC (`result-field-vacuity-closure.md`) | chartered |
| placeholder-typed public signatures | PTYPE (`placeholder-type-eradication-ratchet.md`) | chartered |
| TODO/stub/ignore owners | PTODO | shipped |
| dead public symbols | PDEAD | shipped |
| untested symbols | PUNTESTED | shipped |
| producer-orphan symbols | P1 | shipped |
| doc-chunk ↔ registry truth | PDOCCOVER | shipped |
| `dimensionless_scalar()` reintroduction | PDSSENTINEL | shipped |
| layer-rule imports | PLAYER | shipped |
| PRD terminal-status markers | PPRDSTATUS (#6346) | chartered |
| prose task-promises in doc comments | convention-only — PTODO-grammar-extension bookmark #7098 (filed by `result-field-vacuity-closure.md` ζ) | unenforced |
| whether a declared knob should exist at all | convention-only — deliberately unaudited (`trampoline-param-drop-closure.md` §11) | unenforced |

## INV-SF-1 `undef-has-provenance`

**Rule**: Every *root* undef cell (one whose undef-ness is not inherited
from an undef dependency) carries a recorded `UndefCause`. An unexplained
root undef is itself a defect and surfaces as a diagnostic, never as
silence. Recorded causes are never overwritten with a wrong generic cause.

**Checkable design question(s)**: Can any code path this feature adds
leave a cell Undef without recording why? If a future coverage gap slips
through, does a backstop pass make it visible (a "cell X is undef for an
unrecorded reason" diagnostic), or does it vanish? Does any re-derivation
pass risk overwriting a true recorded cause with a guessed one?

**Evidence**: ~1,350 `Value::Undef` sites in reify-eval/reify-expr vs
~40 cause-recording sites over 5 variants; `trace_undef_causes` returns
empty for unrecorded roots and `cmd_eval` skips empty-cause cells
(reify-cli `main.rs`, undef-notes loop); `reify check` never enables
cause capture; kernel-less re-eval misattributes every geometry undef as
`OpContractViolation` (#5197); #5360 (chained sub reads), #5385
(`generate` of geometry), #5345 (inline geometry query) all silent.

**House pattern**: the undef-self-describing tracer + `UndefCause` origins
map (PRD `docs/prds/v0_6/undef-self-describing.md`); the
`check_no_stale_undef` causeless-staleness checker
(`crates/reify-eval/src/invariants.rs`) — the backstop shape, currently
wired only into the debug-gate corpus harness.

## INV-SF-2 `error-severity-exits-nonzero`

**Rule**: Any Error-severity diagnostic emitted on any channel (compile,
eval, constraint, kernel, build) makes the CLI command exit nonzero. No
per-code bolt-on escalation lists. Corollary (severity hygiene): a
diagnostic *expected* on a healthy path is by definition not
Error-severity — demote or recode it; never exempt it from the gate.

**Checkable design question(s)**: Can this feature print `error:` while
the process exits 0 on any command? Does it add a special-case escalation
for one code instead of relying on the severity gate? Does it emit
Error-severity output on a path that a healthy design can hit (kernel
absent, capability not installed)?

**Evidence**: `reify check` decides exit from constraint outcomes alone
(`finish_check`); eval-phase errors are printed and discarded; escalation
is per-code bolt-on (`GdtIllegalModifier`, `E_DFM_` message-prefix match);
#5386 (non-pub structure error text, exit 0); relate operand-type errors
exit 0.

**House pattern**: `cmd_eval`'s and `cmd_build`'s Severity::Error exit
gates (task 4458) — check must converge on the same rule.

## INV-SF-3 `declared-intent-consumed-or-diagnosed`

**Rule**: Every declaration expressing design intent — a constraint, a
`relate` block, an objective, a DFM rule — is either consumed by a
solve/verify pass this run, or generates a diagnostic naming why not. A
declaration structurally incapable of ever being consumed is a
compile-time error, not a silent no-op.

**Checkable design question(s)**: Enumerate the paths where this feature
drops declared work (filters it out, returns an empty solution, skips on
a missing precondition) — does each emit a diagnostic naming what was
dropped and why? If a user writes the declaration in a place where it can
never take effect, do they find out at compile time?

**Evidence**: `relate` on a scope with no `at auto` sub returns
`RelateSolution::default()` — relations neither solved nor verified
(`crates/reify-eval/src/relate_solve.rs`, zero-auto early return); Bool
autos fall back to the continuous DimensionalSolver because
`SolverRegistry::production()` leaves the Logical slot None while
`cpsat.rs` sits unregistered (`crates/reify-constraints/src/registry.rs`);
minimize over Bool autos builds zero objective components and never
solves; DFM rules with unrealized handles are "silently skipped".

**House pattern**: the #5014 collateral-observability diagnostic (names a
whole unresolved cluster when a merged solve is skipped); the relate
redundant-remainder verify pass — the machinery to verify-instead-of-drop
already exists in `solve_relate_scope`.

## INV-SF-4 `indeterminate-attributable-transient`

**Rule**: `Indeterminate` is reserved for "not measurable in this run,
for a stated runtime reason" (no kernel, below resolution, no
measurement) and carries that reason. A constraint that would be
Indeterminate in every possible run (structurally unresolvable operand)
violates INV-SF-3 and is a compile error.

**Checkable design question(s)**: For each Indeterminate outcome this
feature can produce: what runtime condition clears it, and where is that
reason surfaced? Is there any input for which the constraint is
*permanently* indeterminate — and if so, why is that not a compile error?

**Evidence**: ad-hoc `@`-selector `frame_align` constraints are
permanently INDETERMINATE (inert) under a green check; non-strict check
reports "No constraints violated (N indeterminate)" and exits 0.

**House pattern**: the conservative-refusal discipline in
`engine_constraints.rs` ("degrades to Indeterminate and can NEVER produce
a false Violated") — keep the never-false-Violated half; add the
attributable-reason half. `reify check --strict` promotes indeterminate
to failure.

**Doctrine (ruled by Leo, 2026-08-26, solver-integration session)**: every
`Indeterminate` verdict and every undef output carries a typed cause, and
causes classify as **expected** (the design is declaredly or derivably
abstract at this point in the workflow) or **unexpected** (a mechanism
failed to run or deliver — a solve that never ran, a value that never
flowed, a constraint that never entered the solver problem). Plain
`reify check` fails on unexpected causes; `--strict` continues to gate
all indeterminacy. A newly discovered vacuity class is closed by giving
it a typed cause — never by widening "expected". Delivery vehicles:
declared-intent-consumption-accounting (#5415–#5421) and
eradicate-silent-undef (#5399–#5406). An optional author-side
"abstract here" declaration (complementing a
no-concrete-constraints-style determinacy predicate) is a candidate
follow-on that would let authors pin the expected class explicitly; it is
deliberately optional, not required by the doctrine.

## INV-SF-5 `placeholders-owned-and-loud`

**Rule**: Every placeholder in tracked source — placeholder-typed public
signature (`Real`/`String`/`Length` standing in for a richer type),
placeholder function body, sentinel default — cites a live, non-terminal
task per the PTODO grammar. Blanket escapes that name no owner (the
"awaiting future type-system PRD" pattern) are banned: "no task yet owns
the retarget" must be impossible. Where a placeholder type can silently
accept a wrong argument, it must additionally be loud: a non-matchable
marker/opaque type, or an eval-time misuse diagnostic — statically silent
AND dynamically silent is never acceptable.

**Checkable design question(s)**: Does this feature introduce a public
signature typed with a stand-in (`Real` for a handle, `String` for a
selector) — and if so, which live task owns the retarget, and what
happens today when a caller passes a plausible-but-wrong value? Does any
function body return a value it knows is wrong (typecheck-only body), and
what guarantees the runtime intercept shadows it?

**Evidence**: `flexure_compliance(joint: Length)` — a bare `5mm`
silently overload-matches and yields a sentinel-default record
(`crates/reify-compiler/stdlib/flexures.ri`, joint-type placeholder
block: "no task yet owns" the retarget); mechanism/joint-id `Real`
placeholders across trajectory.ri/dynamics.ri escaped the PTODO gate via
blanket allows; option_recovery.ri/result.ri bodies return incorrect
values, shadowed only by the reify-expr intercept.

**House pattern**: `docs/notes/stdlib-real-placeholder-audit.md` — the
six-bucket census whose task-owned buckets all got fixed (#3111, #3115,
#3116); the `W_FlexureNonJointArg` eval-time misuse warning (task 4547);
the PTODO detector (`docs/prds/reify-audit-ptodo-detector.md` §8) as the
enforcement substrate.

## INV-SF-6 `diagnostics-carry-codes`

**Rule**: Every emitted Warning/Error carries a `DiagnosticCode`.
Code-less diagnostics cannot be gated, filtered, counted, or de-noised
systematically, and force message-substring hacks downstream.

**Checkable design question(s)**: Does this feature emit any diagnostic
without a code? Does any consumer it adds match on message text where a
code should exist?

**Evidence**: 362 `Diagnostic::error/warning` ctor sites in reify-eval,
67 with codes; the CLI's `E_DFM_` message-prefix escalation exists only
because co-resident Error diagnostics are code-less.

**House pattern**: `DiagnosticCode` registry + typed-code test assertions
(tasks 2255, 3416 flipped substring tests to code identity).

## INV-SF-7 `parse-is-value-faithful`

**Rule**: No grammar or parser ambiguity may silently change an
expression's value. Where the grammar admits more than one reading of a
token sequence (e.g. quantity-literal juxtaposition absorbing a
following line), the resolution is a parse error at the ambiguity site —
never a quiet pick. A misparse that yields a well-typed WRONG value is
the worst silent-failure shape: it satisfies SF-1..SF-6 while the number
is garbage.

**Checkable design question(s)**: Does this feature add grammar that
composes with quantity-literal juxtaposition or any other
adjacency-sensitive rule? If so, does it carry ambiguity-regression
corpus tests pinning that adjacent-token variation cannot change a
parsed value? Can any statement/expression boundary in the new grammar
absorb a following line without a diagnostic?

**Evidence**: #5392 (functional-enumeration probe 2026-07-24): a fn body
`let x0 = cos(0deg)` followed on the next line by `x0 * sgn(i, 0)` — no
semicolon — returned -1 where +1 is correct, with zero diagnostics; the
same seam yields four outcomes (correct / silently wrong / undef /
parse-error-at-wrong-line) depending on adjacent tokens. #5492's corpus
red on main is suspected drift of the same seam. Ratified by Leo
2026-07-25 (seam review); #5392 is the enforcement vehicle for the
fn-body seam.

## Angle-crossing family (INV-AD-1..4)

> Angle is primitive; the differential and tensor algebra is
> quotient-pure; every angle reading of a geometric ratio is an
> explicit named crossing carrying η = 1 rad.
> — Leo, ratified 2026-08-10.

The law has two operational halves: **quotient purity** (no operator
over fields or tensors ever manufactures `rad` from a derivative) and
**named crossings** (`rad` enters only at a named primitive, channel, or
helper that asserts an arc measure). It generalizes rulings already
made in this territory: #6164 (A+ — `ElasticResult.rotation` = curl/2
is the designated crossing), #6080 (`orient_log` is a genuine
arc-measure primitive, not a quotient), #6126/#6089 (translation stays
Length, never swept into angle), #5785 (η — TORQUE = N·m/rad), #5799
(rotational stiffness re-dimensioned to rad⁻²), and #5825→#5844 (moment
of inertia carries rad⁻²).

**Registry alias.** `docs/invariants.md` rolls this family up under a
single row, INV-DIM-1 (Enforcement `doc+test`) — an id-keyed audit
resolves INV-DIM-1 and INV-AD-1..4 to the same doctrine; INV-DIM-1
names no fifth invariant of its own.

## INV-AD-1 `angle-crossings-explicit`

**Rule**: Any proposal that gives a derived or geometric ratio an Angle
type is wrong unless it introduces `rad` at a named primitive, channel,
or helper. A bare quotient producer never yields Angle on its own.

**Checkable design question(s)**: Does this feature type anything Angle
whose producer is a quotient — and if so, which named crossing asserts
the arc measure? Is there any path to an Angle-typed value in this
feature that does not pass through a named primitive/channel/helper?

**Evidence**: inverse trig and `phase`/`arg` return `Type::angle()` at
the static layer (`crates/reify-compiler/src/math_signatures.rs:357,
:375`) and tag `DimensionVector::ANGLE` at eval
(`crates/reify-stdlib/src/trig.rs:20-35`); the geometry queries `angle`
and `angle_between_surfaces` are the same shape — `Type::angle()` at
`crates/reify-compiler/src/units.rs:349` (`angle_between_surfaces`) and
`:1308` (`angle`), tagged `DimensionVector::ANGLE` at
`crates/reify-compiler/src/relation_signatures.rs:394` — every shipped
Angle producer is a named site, never a bare quotient.

**House pattern**: `asin`/`atan2` (shipped,
`crates/reify-stdlib/src/trig.rs:20-35`); `orient_log`
(`crates/reify-stdlib/src/orientation.rs:203`) as the ruled-pending
exemplar (#6080) — a 2·atan2 arc-measure primitive, not a quotient, not
yet Angle-tagged at eval; and #6164's `ElasticResult.rotation` = curl/2
channel, cited as RULED, PENDING (#6164) — confirmed absent from
`crates/reify-compiler/stdlib/solver_elastic.ri` today, so never
describe it as shipped.

## INV-AD-2 `quotient-pure-derivative-algebra`

**Rule**: No operator over fields or tensors ever manufactures `rad`
from a spatial or temporal derivative. Operator registry rows
(gradient, divergence, curl, laplacian) stay pure quotient — basis
`Ruling("#6164")`.

**Checkable design question(s)**: Does this feature add or retype an
operator row such that `rad` appears in a codomain the domain did not
carry?

**Evidence**: `dim_quotient_type` / `differential_codomain`
(`crates/reify-core/src/field_calculus.rs:84+`), consumed by every
differential-operator arm (`crates/reify-compiler/src/units.rs:1190`
gradient, `:1204` divergence) — verified rad-free: the result dimension
is always the codomain/domain quotient, never a named angle unit.

**House pattern**: `differential_codomain` itself is the standing
precedent for quotient purity — the shape every future field/tensor
operator row copies.

## INV-AD-3 `tensor-single-quantity`

**Rule**: Never propose per-component or per-block tensor dimensions. A
tensor carries exactly one quantity slot; an angle reading of a tensor
is extracted by a named helper, never by giving individual components
their own dimension.

**Checkable design question(s)**: Does this feature want a tensor whose
components carry differing dimensions, or an in-place angle reading of
a tensor — and if so, which named extractor delivers it?

**Evidence**: `Type::Tensor { rank, n, quantity: Box<Type> }`
(`crates/reify-core/src/ty.rs:282-286`) carries one quantity slot, so
mixed per-component dimensions are UNREPRESENTABLE by construction —
even though rotation covariance would want exactly that. Authors have
no in-language tensor component access (`IndexAccess` and member access
both reject tensors), so a named extractor is structurally forced, not
merely a style preference.

**House pattern**: `ElasticResult.shear_angles` (chartered, PRD leaf σ)
beside `.rotation`, both built on the `sampled_curl_field` mechanics
(`crates/reify-eval/src/compute_targets/mod.rs:166`).

## INV-AD-4 `boundaries-declare-angle-convention`

**Rule**: Any boundary that carries angular values out of or into
reify — a process boundary, file format, FFI call, or wire protocol —
names its convention (rad / deg / cycles) in a greppable contract
comment, schema text, or refusal guard. A silent rad=1 SI erasure is a
defect even when the number it produces is correct.

**Checkable design question(s)**: Does this feature move an angular
value across a process, file-format, FFI, or wire boundary — and where
is the convention declared? If undeclared today, is that named as a gap
rather than assumed correct by silence?

**Evidence**: rad=1 erasure at `as_f64` / `read_scalar_si` is correct SI
behaviour; the defect is that no boundary declares it — STEP, OCCT
`angle_rad`, SolveSpace `angle_deg`, MCP `set_parameter`, and GUI
channels all carry angular values today with no declared convention
(chartered as PRD leaves ι/υ).

**House pattern**: the `spring_rate_for_lumped_dof` refusal guard
(`crates/reify-eval/src/modal_ops.rs:1300`) and
`eigenvalue_to_frequency_hz` as the declared-bridge precedents.

### Crossing catalogue and identities

**Canonical copy: this section.** `docs/prds/v0_6/angle-dimension-completion.md`
§4 carries a near-identical table as its own content contract for this
doctrine — D6 charters the doctrine landing in *both invariant
surfaces* (this file and `docs/invariants.md`), not a third copy in
the PRD. Treat the PRD's table as derived: update the catalogue here
first, and refresh the PRD copy in the same change if it is touched.
The Status column below is mutable by design as sibling leaves (α, σ,
ι, υ, and #6080/#6164) land — exactly the condition under which two
unmarked copies would silently diverge.

**The crossing catalogue** — every named site where η = 1 rad enters,
its mechanism, and an honest shipped / ruled-pending / chartered
status:

| Crossing | η enters at | Mechanism | Status |
|---|---|---|---|
| inverse trig / `phase` / `arg` | return type | `math_signatures.rs` + eval Angle tag | shipped |
| `orient_log` / rotation vectors | return type (2·atan2 primitive) | #6080 | ruled, pending |
| geometry `angle`, `angle_between_surfaces` queries | return type | query typing | shipped |
| geometry `curvature` query | return type (dθ/ds primitive) | angle-dimension-completion leaf α | chartered |
| `ElasticResult.rotation` = curl/2 | named channel | #6164 | ruled, pending |
| `ElasticResult.shear_angles` | named channel | angle-dimension-completion leaf σ | chartered |
| MOI kernel seam (∫ρr²dV → rotational inertia) | deliberate rad⁻² tag at `dispatch_inertia_tensor` | #5825 ruling, #5844 implementation | ruled, pending |
| joint-DOF unwrap; unit literals (`45deg`, `1rad`) | literal/decode sites | unit-literal lowering + joint-DOF decode sites | shipped |
| hand-rolled `.ri` crossings | `expr * 1rad` (and `expr / 1rad` to leave) | unit arithmetic (probed) | shipped, undocumented (corpus example → leaf γ) |
| frequency ↔ angular frequency | 2π rad/cycle — a distinct constant, not η (see below) | FREQUENCY ≠ ANGULAR_VELOCITY forces it in `.ri`; Rust marshalling boundaries stay f64 with declared comments | shipped (typed layer), declaration chartered as leaf ι |
| IO boundaries (STEP, FFI, MCP, GUI) | rad=1 SI erasure — correct numerically, must be declared | angle-dimension-completion leaves ι/υ | chartered |

**Textbook identities under the law** (the quotient algebra yields the
η-free right-hand side; a named helper or the author's `* 1rad` supplies
the crossing):

- s = rθ/η
- v = (ω×r)/η
- r = η/κ
- γ ≈ η·(∂u_x/∂y + ∂u_y/∂x)
- H = η·∇²φ/2 (future helper — greenfield; no code computes this today)
- κ = η·|dT/ds| (future helper — greenfield; no code computes this today)

**The #5825 arc-length discharge.** Task #5825 (done) carved out
exactly this case: *"`arc = r * theta` evaluating to `m·rad` rather
than Length. Irreducible under any radian-as-dimension scheme without
an explicit conversion constant, broken today, unaffected by this
ruling. Needs its own task — do NOT fold it in."* This family is that
task, for the teaching half only. The idiom is `r * theta / 1rad`: to
enter a crossing, multiply by `1rad`; to leave one, divide by `1rad` —
always the **no-space** literal form, `1rad`. The spaced form `1 rad`
is a parse error (probe-verified) — deliberate, not a gap: see INV-SF-7
`parse-is-value-faithful`, which owns the quantity-literal juxtaposition
seam that makes `1rad` vs `1 rad` a meaningful distinction rather than
whitespace noise. No enforcement is possible or chartered for the
arc-length case itself — the teaching above is the whole deliverable.
Executable substrate: the committed fixture
`tests/prd-gate/fixtures/angle_crossing_idiom.ri` (read-only here; leaf
γ owns its probe row).

### The 2π rad/cycle distinction (D4)

Frequency ↔ angular frequency is its own crossing class, **not** η.
ω = 2π·f·(1rad) carries 2π rad/cycle — a distinct constant from the
η = 1 rad crossing above; never conflate the two. The typed layer
already forces the distinction: FREQUENCY ≠ ANGULAR_VELOCITY in `.ri`,
so an author cannot silently swap one for the other. Rust-side f64
marshalling boundaries (where the typed distinction does not reach)
stay f64 and gain a doctrine-citing comment instead of a retype
(chartered as leaf ι) — declaring the convention, not converting the
representation.

### ANGULAR_MOMENTUM: algebra without a chartered constant (D3, §11)

L = Iω = J·s/rad, and τ = dL/dt closes to TORQUE (probed against the
#5844 rad⁻² MOI). This algebra is recorded here so a future task does
not have to re-derive it — but no ANGULAR_MOMENTUM constant is
chartered by this family: it has zero producers and zero consumers
anywhere in the repo today, and minting a dimension with neither would
create a pure orphan. Promote it to a chartered constant when a
producer or consumer materializes.

### Enforcement honesty (D7)

The crossing idiom is **teachable but not yet mandatory**. `sin(2.5)`
and `param x : Angle = 2.5` both pass silently **today**, for two
unrelated reasons — nothing in this family changes that:

- **Literal-only, `param`/`let` only**: a bare or negated
  dimensionless literal widens into a dimensioned `Scalar` param
  default or `let` annotation (`is_numeric_literal_expr`,
  `crates/reify-compiler/src/entity.rs:390`; guards at `:479-485`
  and `:563-569`). A dimensionless *expression* at the same boundary
  is rejected instead: `param x : Angle = ratio * 2.0` is a `BinOp`,
  so it falls through to `type_compatible` — which carries no
  dimensionless→dimensioned rule of its own
  (`crates/reify-compiler/src/type_compat.rs:220-237`) — and errors
  `ParamDefaultTypeMismatch` (`let` twin:
  `LetAnnotationTypeMismatch`). The carve-out never reaches function
  param defaults: those use strict equality
  (`fn_param_default_compatible`), so even
  `fn f(a : Angle = 2.5)` errors `FnParamDefaultTypeMismatch`
  (`crates/reify-compiler/src/functions.rs:187-215`).
- **`sin(2.5)` isn't a widening — trig arguments are unchecked**:
  no argument-dimension check exists for the transcendental family.
  `MATH_TRANSCENDENTAL_NAMES` is a name list only
  (`crates/reify-compiler/src/math_signatures.rs:95-97`); its
  "accept ANGLE-or-Real" comment is intent only, not enforced.
  `math_fn_result_type` fixes just the RESULT type
  (`crates/reify-compiler/src/math_signatures.rs:374`) and never
  inspects the argument, so every argument passes: `sin(2.5)`,
  `sin(ratio * 2.0)`, and `sin(5.0mm)` (LENGTH) all compile clean.

Mandatoriness is owned elsewhere: the
real-dimensionless-unification decision D5 (current ruling:
`docs/prds/v0_6/dimensioned-construction-strictness.md` §0 — D5's own
three-position reversal history is at that decision's "D5 status"
section), and the
*angle-units-surface-convergence* PRD's own leaves β/γ/δ/ε (unlanded —
a distinct PRD from this family's angle-dimension-completion
programme, with its own independent leaf lettering). No enforcement
work is chartered by this family; those siblings are cited by name
only — their contracts are not restated here. The enforcement
*direction* this doctrine cites is strict dimension equality
(units-length-gate-completion D11, #5747).

## Declared-surface family (INV-PD-1..2)

The input and output halves of one contract: a declared surface either
does what it declares, or names the live task that owns making it do
so. Both are instances of the umbrella principle above; both are
declared-not-inferred (inference cannot see indirection — the
esc-6739-1 aliasing blind spot) and both make owner liveness a gate
failure mode, not a convention.

## INV-PD-1 `declared-param-reaches-kernel`

**Rule**: Every `param` declared on a structure a trampoline consumes
is *honored* (its value reaches the kernel and changes the result),
*declared-ignored* with a live owning task (setting it to a
non-default value is `E_PARAM_NOT_HONORED`, carrying the owner), or
*not_applicable* with a one-line reason (`W_PARAM_NOT_APPLICABLE`).
The three sets are disjoint and their union equals the
`structure_def`'s param set. Enforcement: `reify-audit --pattern
PDROP`, which also reds on dead/absent owners.

**Checkable design question(s)**: Does this feature add or consume a
`structure_def` param its trampoline does not read — and if so, which
bucket does it declare, and which live task owns the honor? Can a
caller set a knob and get a plausible-but-wrong number with exit 0?

**Evidence**: the 2026-08-31 drop census — ten answer-changing drops
(`solve_buckling` discarding required `supports`; `ElasticOptions.max_iter`
declared 1000, hardcoded 2000; a `force_limit` read on a field no
`.ri` structure declares; …). Normative contract, census and
decomposition: `docs/prds/v0_6/trampoline-param-drop-closure.md`
(C1–C4 there; not restated here).

**House pattern**: #4149's `buckling_unsupported_option_diagnostics` /
`DiagnosticCode::BucklingOptionUnsupported` — complete within
`BucklingOptions`, the shape PDROP universalizes.

## INV-PD-2 `result-fields-populated-or-owned`

**Rule**: Every field a Rust producer writes into a `.ri`-declared
result structure is *populated* (real, sampleable, non-sentinel value
on the production path), *degraded* (the honest `Undef` form plus a
recorded reason), or *allowlisted* with a live owning task. The three
sets are disjoint and their union equals the `structure_def`'s
declared field set; a written-but-undeclared field is itself a
finding. A degraded field holds `Undef` (with an `UndefCause` where
the channel exists, INV-SF-1) — never a plausible well-formed fake.
Enforcement: `reify-audit --pattern PVAC`, which also reds on
dead/absent owners.

**Checkable design question(s)**: Does this feature declare a result
field its producer leaves empty or sentinel — and if so, which bucket,
and which live task owns populating it? Does it write a field the
`.ri` does not declare? Can a reader mistake a degraded value for a
computed one (well-formed empty instead of `Undef`)?

**Evidence**: the 2026-08-31 vacancy census — `ModalResult.part`
populated by `placeholder_part()` (a well-formed zero-field fake) with
zero readers and its growth promise citing done #4578; the undeclared
engine-attached `ModalResult.topology`, always `Undef` on the dims
path; `mechanism_modal`'s `shape = []` / `participation_mass = 0`
(#7012 owns the degraded reason). Normative contract, census and
decomposition: `docs/prds/v0_6/result-field-vacuity-closure.md`
(C1′–C4′ there; not restated here).

**House pattern**: the tet-result `Undef`-for-unpopulated convention —
buckling `pre_stress`, `degenerate_modal_result`'s `damping` — is C2′
promoted to contract.

## Census seam

Reify's confusion-codebook entries
(`docs/legibility/confusion-codebook.yaml`) MAY carry
`invariant_violated: <slug>`; the slug vocabulary is this doc. A slug
violated repeatedly across census batches is an enforcement gap: file a
guard task.
