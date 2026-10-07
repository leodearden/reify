# PRD: Angle-dimension completion programme (the crossing law)

**Milestone:** v0_6 · **Status:** active (authored 2026-08-10, HEAD `9ca5c6ad9f`) ·
**Approach:** B + H (the doctrine section *is* the contract) · **Programme codename:** P

**Provenance.** Chartered from the 2026-08-07 type-decision enshrinement review
(`~/.claude/fleet/sessions/review-reify-867571/report.md`) and its 2026-08-10 adjudications.
Every load-bearing claim below was re-verified against HEAD `9ca5c6ad9f` by a six-agent
investigation in the chartering session; premises inherited from the spawn brief that turned
out false are corrected inline with **[corrected]**.

---

## §0 — The law

> **Angle is primitive; the differential and tensor algebra is quotient-pure; every angle
> reading of a geometric ratio is an explicit named crossing carrying η = 1 rad.**
> — Leo, ratified 2026-08-10.

This one rule generates everything already ruled in this territory, and this PRD's job is to
complete the programme those rulings instantiate locally:

- **#6164 (A+)** — `ElasticResult.curl` stays `Vector3<Dimensionless>` *as a decision*;
  `ElasticResult.rotation : Vector3<Angle>` = curl/2 is the designated crossing.
- **#6080** — `orient_log` returns `Vector3<Angle>`: it computes 2·atan2, a genuine
  arc-measure *primitive*, not a quotient.
- **#6126 / #6089** — Twist.linear and Frame/Transform/AffineMap translation are Length (D11
  strict-equality direction).
- **#5799 (done) / #5825→#5844 (pending)** — TORQUE = N·m/rad, ROTATIONAL_STIFFNESS/DAMPING
  rad⁻², MOMENT_OF_INERTIA → kg·m²/rad².

The law's two halves in operational terms:

1. **Quotient purity.** No operator over fields/tensors ever manufactures `rad` from a
   spatial or temporal derivative (`differential_codomain`,
   `crates/reify-core/src/field_calculus.rs:85-110` — verified rad-free). Tensors carry one
   quantity slot (`crates/reify-core/src/ty.rs:281-286` — mixed per-component dimensions are
   *unrepresentable*; rotation covariance requires this).
2. **Named crossings.** `rad` enters only at named primitives/channels/helpers that assert an
   arc measure: inverse trig and `phase`/`arg` (`math_signatures.rs:357,:375`; eval tags
   verified in `reify-stdlib/src/trig.rs:20-35`), `orient_log`, the geometry `angle` and
   (after this PRD) `curvature` queries, joint-DOF unwrap, unit literals, the `.rotation`
   channel, and hand-rolled `.ri` crossings written `expr * 1rad` (substrate-probed: parses,
   checks, and evaluates today; the spaced form `1 rad` is a parse error).

## §1 — Goal + consumers (G1)

After this PRD: the curvature a `.ri` author queries is honestly `rad/m`; the rotational
dynamics algebra closes and is demonstrated in a CI-gated example; the angle readings of the
strain field are reachable through a named Angle channel; the crossing doctrine is written
where machines and authors both read it; and every boundary where angular values leave or
enter reify carries a *declared* convention instead of an accidental one.

| Mechanism | Consumer | Surface |
|---|---|---|
| CURVATURE = rad·m⁻¹ | registry τ8 (#6010) curvature-query rows (edge wired at decompose); `.ri` authors comparing curvature against `deg/mm` quantities; the doctrine doc | `reify check`/`eval` typing + display `rad·m^-1` |
| ANGULAR_ACCELERATION named dimension | names τ/I in eval output; the closure exemplar; future typed q̈ (`Rate<AngularVelocity>` already composes the vector — this adds the *name*) | `reify eval` prints `AngularAcceleration`; `param a : AngularAcceleration` resolves |
| Rotational-closure exemplar | `.ri` dynamics/flexure authors (the k·θ / τ=Iα / E=½Iω² family); the ω-vs-f 2π teaching | CI-compiled example; deg-literal comparison |
| `ElasticResult.shear_angles` channel | FEA authors checking shear-strain limits (worked example ships as the consumer, same-diff); mirrors #6164's `.rotation` | `sample(result.shear_angles)` → `fn f(v: Vector3<Angle>)` vs deg literals in CI |
| Crossing doctrine (invariants row + design-invariant family) | `/prd` G7 walks and `/review` phase 2 (auto-read `docs/legibility/design-invariants.md`); registry row `basis` cites (I-REG-7); τ1 (#6003)'s ratified `x/1rad` diagnostic hints; task #5825's arc-length carve-out ("`arc = r·θ` … needs its own task" — the teaching half lands here) | committed doctrine files; G7-walkable questions |
| Docs-truth four-pack | `.ri` authors discovering the idiom by intent; the reify-design skill | chunk section, corpus example, SKILL.md index line, discoverability transcript |
| IO bridge declarations | STEP consumers; MCP clients; SolveSpace/OCCT maintainers; PRD-5's reader work | declared unit conventions, grep-able contract comments |
| GUI channel bridge (unit tag + signed range) | #6164's `.rotation` channel (first signed, first Angle channel — today the wire format has no unit slot and the range clamp drops negatives); existing Pa channels gain honest legends | GUI legend shows the channel's unit; signed channel renders |

No new in-engine seam: the shear channel rides the existing ComputeNode trampoline
(overlay G1 §3.4, same as #6164's `.rotation`); everything else is dimension-layer, docs, or
declared-boundary work.

## §2 — Premise verification (what is true at `9ca5c6ad9f`)

**The "halfway state" claim stands.** Slot 7 = Angle with rational exponents
(`dimension.rs:103-130`); TORQUE/ROTATIONAL_* carry their ruled vectors (`:339`, `:283`,
`:293`); ANGULAR_VELOCITY ≠ FREQUENCY (`:187`/`:143`); inverse trig returns Angle at both
static and eval layers; `Rate<Q>` is live (`units.ri:116`). Meanwhile: MOMENT_OF_INERTIA is
still kg·m² (**#5844 pending** — today's algebra provably does not close: probed,
½Iω² → `m²·kg·rad²·s⁻²`, matching nothing), CURVATURE is `m⁻¹` sharing its vector with
ABSORPTION_COEFF, and the crossing idiom appears in zero docs.

**Brief corrections [corrected]:**
1. **Torsion has an empty referent.** No Frenet-torsion quantity, query, or constant exists
   anywhere; every in-repo "torsion" is torsional *stiffness* prose. Torsion drops to
   doctrine (§4) — declared rad/m *when introduced*, no code change now.
2. **The claimed η-insertion sites inside vector-calculus computations do not exist yet.**
   `medial.rs` computes only ∇φ (no mean curvature); the `∇²φ ≈ 2H` path exists only as
   dimensionless test-only acceptance in reify-expr; no code computes |dT/ds| or Frenet
   identities. The crossings are *greenfield future sites the doctrine specifies*, not
   retrofits.
3. **Moment of inertia is not new scope** — ruled (#5825, done) and owned (#5844, pending).
   This PRD consumes it and charters only the unowned residue (ANGULAR_ACCELERATION name +
   closure exemplar). **Angular momentum is surfaced nowhere**, typed or raw — chartering it
   now would mint a pure orphan; deferred (§11) with its algebra recorded in doctrine
   (L = Iω = J·s/rad, τ = dL/dt closes — probed).
4. **There is no stored strain tensor** — strain is the symmetric part of
   `ElasticResult.gradient`. The shear scope item is phrased against `gradient`. Authors
   have *no* tensor component access (verified: IndexAccess and member access both reject
   tensors), so a named extractor is the only possible reading path — the law's "extracted
   via named helpers" clause is structurally forced, not merely preferred.
5. **The Hz↔rad/s crossing carries 2π rad/cycle, not η = 1 rad** — a distinct crossing
   *class* (frequency ↔ angular frequency), named separately in doctrine. The claimed
   Rust-side conversion sites are real (`input_shape.rs:116-126`, `free_vibration.rs:25-31`)
   and already partially declared.
6. **"Length crossing to OCCT is the model bridge" is inverted.** No m→mm conversion or STEP
   unit declaration exists; STEP/3MF exports write SI-metre coordinates under a
   `.MILLI.METRE.` declaration — a live 1000× interop defect (probe-verified), filed as a
   standalone bug task at decompose, **not** owned by this PRD (§11). The real declared-bridge
   precedents are `eigenvalue_to_frequency_hz`, the ZVShaper marshalling boundary, the fdm
   m→mm comment, and the `spring_rate_for_lumped_dof` refusal guard.
7. **Enforcement honesty.** The crossing idiom is teachable but not yet mandatory:
   `sin(2.5)` and `param x : Angle = 2.5` pass silently today (bare dimensionless widens into
   any declared dimension). Mandatoriness is owned by the units-gating siblings
   (real-dimensionless-unification D5; angle-units PRD leaves β/γ/δ/ε, unlanded). The
   doctrine must state this rather than imply enforcement.

## §3 — Sketch of approach

Five workstreams, five-plus-two leaves (§10):

1. **Curvature** (α): re-dimension `CURVATURE` m⁻¹ → rad·m⁻¹. Curvature is definitionally
   dθ/ds with θ primitive — rad/m natively, no η at the definition; the geometry `curvature`
   query becomes an arc-measure primitive exactly like `orient_log` (OCCT computes the
   arc-measure rate; η enters *inside* the query tagging, numerically ×1). Blast radius is
   small and fully enumerated (§6): one constant + doc, one literal pin test, two hand-built
   duplicate consts (the production one gets a drift-pin test), doc corrections, two recorded
   decisions re-adjudicated (task-3603 alias rationale; the "curvature precedent" wording in
   `differential-field-operators.md`). Values are byte-identical (η = 1). Side benefit: the
   CURVATURE/ABSORPTION_COEFF vector collision dissolves — curvature becomes the sole holder
   of rad·m⁻¹ with its own canonical name, the same shape as #5799's TORQUE resolution.
2. **Dynamics residue** (δ): `ANGULAR_ACCELERATION` (rad/s²) named dimension + the
   rotational-closure exemplar, dep-wired on #5844. The exemplar is the two-way boundary test
   of the whole dimension algebra (§9).
3. **Shear-angle channel** (σ): `ElasticResult.shear_angles : Field<Point3<Length>,
   Vector3<Angle>>` — the Voigt off-diagonal engineering shears (γ_yz, γ_zx, γ_xy) = doubled
   symmetric off-diagonals of ∇u, ×η, valid in the small-deformation regime (same proviso as
   `.rotation`). Implemented with the verified #6164 mechanics: a wrap-time linear projection
   of the stored `nodal_gradient` beside `sampled_curl_field`, Angle codomain declared in
   `.ri`, runtime tag following from the declared codomain (`sampled.rs:140-143,:297-305` —
   zero new sampling machinery). Ships with its worked consumer example same-diff (§1).
4. **Doctrine + docs** (β, γ): the law into `docs/invariants.md` (registry row, enforcement
   `doc+test`) **and** a new G7-walked family in `docs/legibility/design-invariants.md`
   (INV-AD-*), plus the docs-truth four-pack.
5. **IO bridges** (ι, υ): declare the angular conventions at every boundary that carries
   angular traffic today (STEP cone semi-angle, OCCT `angle_rad` FFI, SolveSpace degrees,
   MCP inbound radians, dynamics erasure sites), and give the GUI channel wire format a
   unit/dimension tag + signed-range handling before #6164's `.rotation` becomes the first
   signed Angle channel.

## §4 — Contract: the crossing doctrine (G5, approach H)

The normative text lands in β's two artifacts; this section is its content contract.

**The crossing catalogue** (each row: where η enters, mechanism, status):

| Crossing | η enters at | Mechanism | Status |
|---|---|---|---|
| inverse trig / phase / arg | return type | `math_signatures.rs` + eval Angle tag | shipped |
| `orient_log` / rotation vectors | return type (2·atan2 primitive) | #6080 | ruled, pending |
| geometry `angle`, `angle_between_surfaces` queries | return type | query typing | shipped |
| geometry `curvature` query | return type (dθ/ds primitive) | **this PRD α** | chartered |
| `ElasticResult.rotation` = curl/2 | named channel | #6164 | ruled, pending |
| `ElasticResult.shear_angles` | named channel | **this PRD σ** | chartered |
| MOI kernel seam (∫ρr²dV → rotational inertia) | deliberate rad⁻² tag at `dispatch_inertia_tensor` | #5825 ruling, #5844 implementation | ruled, pending |
| joint-DOF unwrap; unit literals (`45deg`, `1rad`) | literal/decode sites | shipped | shipped |
| hand-rolled `.ri` crossings | `expr * 1rad` (and `expr / 1rad` to leave) | unit arithmetic (probed) | shipped, undocumented → γ |
| frequency ↔ angular frequency | ω = 2π·f·(1rad)... i.e. **2π rad/cycle — a distinct constant, not η** | FREQUENCY ≠ ANGULAR_VELOCITY forces it in `.ri`; Rust marshalling boundaries stay f64 with declared comments | shipped (typed layer), declaration → ι |
| IO boundaries (STEP, FFI, MCP, GUI) | rad = 1 SI erasure — *correct* numerically, must be *declared* | ι/υ | chartered |

**Textbook identities under the law** (doctrine teaches these honestly): s = rθ/η ·
v = (ω×r)/η · r = η/κ · γ ≈ η·(∂u_x/∂y + ∂u_y/∂x) · H = η·∇²φ/2 (future helper) ·
κ = η·|dT/ds| (future helper). The quotient algebra yields the η-free right-hand sides;
the named helper or the author's `* 1rad` supplies the crossing.

**Closure identities** (δ's exemplar pins these end-to-end, dimensions probed):
E = ½Iω² → J · τ = Iα → N·m/rad · L = Iω → J·s/rad · dL/dt → TORQUE · k·θ → TORQUE ·
½kθ² → J.

**Invariant family sketch** (final wording is β's deliverable, slugs stable once landed):
- `INV-AD-1 angle-crossings-explicit` — any proposal giving a derived/geometric ratio an
  Angle type is wrong unless it introduces rad at a named primitive/channel/helper.
  *Checkable question:* does this feature type anything Angle whose producer is a quotient?
- `INV-AD-2 quotient-pure-derivative-algebra` — no operator inserts rad; operator registry
  rows stay pure quotient (basis `Ruling("#6164")`).
- `INV-AD-3 tensor-single-quantity` — never propose per-component/per-block tensor
  dimensions; angle readings of tensors are extracted by named helpers.
- `INV-AD-4 boundaries-declare-angle-convention` — a boundary crossing angular values names
  its convention (rad/deg/cycles) in a greppable contract comment, schema, or refusal guard;
  silent rad=1 by accident is a defect even when numerically correct.

## §5 — Resolved design decisions

- **D1 — CURVATURE re-dimensions to rad·m⁻¹.** Basis: κ = dθ/ds with θ primitive — the
  *query* asserts an arc measure (the #6080/orient_log shape); the operator algebra stays
  untouched. This **supersedes the task-3603/GHR-α alias rationale** ("no physically honest
  way to make them distinct", `dimension.rs:250-253`) — under angle-as-dimension there is
  one: curvature is rad·m⁻¹, absorption stays m⁻¹. α rewrites that doc comment as a
  re-adjudication record. It also retires the *wording* (not the substance) of #6164's
  "curvature precedent, m⁻¹ with no rad" exemplar: the standing precedent for quotient
  purity is `differential_codomain` itself. α amends
  `differential-field-operators.md` (:17/D6) accordingly; #6164's own task text carries the
  same stale exemplar and is flagged for Leo's sign-off (ruling-protected — see §8).
- **D2 — No torsion work.** Empty referent (§2.1). Doctrine declares Frenet torsion rad/m
  prospectively.
- **D3 — Dynamics: consume, don't re-charter.** #5844 owns MOI; #5799 landed stiffness/
  damping; this PRD adds only ANGULAR_ACCELERATION (the name for τ/I — `Rate<AngularVelocity>`
  already composes the vector) and the closure exemplar. ANGULAR_MOMENTUM deferred (§11) —
  zero producers/consumers anywhere; chartering it would mint an orphan (G1).
- **D4 — 2π rad/cycle is its own crossing class.** The doctrine names it separately from
  η = 1 rad; the typed layer already forces it (FREQUENCY ≠ ANGULAR_VELOCITY); Rust-side f64
  marshalling boundaries get doctrine-citing comments (ι), not retypes.
- **D5 — Shear angles: channel, not builtin, one triple.** σ ships the `ElasticResult`
  channel using #6164's exact mechanics (already verified end-to-end). A generic
  `shear_angles(Tensor<2,3,Real>)` analysis builtin (the `stress_invariants`
  dimension-transforming precedent) generalizes but has no consumer today → §11 bookmark.
  Convention: Voigt order (γ_yz, γ_zx, γ_xy), engineering shear (γ = 2ε_offdiag), small-
  deformation proviso in the doc comment mirroring `.rotation`'s.
- **D6 — Doctrine lands in both invariant surfaces.** `docs/invariants.md` row (INV-DIM
  family, enforcement `doc+test`: the closure exemplar + curvature pins are the test half) +
  `docs/legibility/design-invariants.md` INV-AD family (checkable questions, auto-walked by
  /prd G7 and /review — the strongest available enforcement lever for *future* PRDs). The
  design-invariants doc explicitly invites appended families with stable slugs.
- **D7 — The doctrine states its own enforcement status.** Idiom-teaching now; mandatoriness
  arrives with the units-gating siblings (§2.7). No enforcement work is chartered here — G4:
  that territory is owned (angle-units PRD β/γ/δ/ε; real-dimensionless-unification D5).
- **D8 — IO bridges are declarations, not conversions.** rad = 1 erasure at `as_f64`/
  `read_scalar_si` is numerically correct SI behaviour; the defect is that no boundary
  *declares* it. ι stamps the conventions (STEP PLANE_ANGLE radian; OCCT `angle_rad`
  contract; SolveSpace `angle_deg` degrees; MCP `set_parameter` inbound-SI-radians schema
  text; dynamics `compliance_cell_f64` erasure comment pointing at PRD-5 territory). The
  1000× STEP/3MF *length* mislabel is filed as its own bug task (§11).
- **D9 — Docs-truth content is disjoint from the angle-units PRD's pending docs leaves.**
  #5790 (ξ) owns the angle-surface/torque-spelling teaching in `chunks/units.md`; γ owns a
  *new* crossing-doctrine section in the same file. #5792 (ο) owns
  `best_practices/angles_and_torque.ri`; γ adds a separate `angle_crossings.ri`. Same files,
  different sections/rows → file-lock serialization only, no dependency edges, no content
  contradiction (γ cites, never restates, ξ's spelling teaching).
- **D10 — GUI channel bridge lands as a tagged wire format.** `scalar_channels:
  HashMap<String, Vec<f32>>` gains a per-channel unit/dimension tag, and the range
  computation stops clamping negatives for signed channels. Chartered *now* so #6164's
  `.rotation` (the first signed, first Angle channel) lands on a bridge instead of arriving
  as unmarked clamped radians; the existing Pa channels get honest legends for free.

## §6 — Premise validation (G6)

- **α signals.** Probe evidence: `1deg / 1mm` already evaluates to `17.45… rad·m⁻¹` and
  renders `rad·m^-1` (composed display); `param k : Curvature = 0.2 / 1mm` **passes today**
  and must **fail post-change** (dimensioned-vs-dimensioned initializer mismatch — the
  rejection mechanism is live, control-probed: `param t : Angle = 5mm` errors, exit 1);
  `param k : Curvature = 0.2rad / 1mm` fails today and must pass post-change. Zero corpus
  `: Curvature` declarations exist → no migration. All OCCT numeric pins are raw f64 —
  byte-identical (η = 1). The literal pin `curvature_constant_is_length_inverse`
  (`dimension.rs:1909-1931`) flips deliberately. **Silent-divergence hazard bound:** the
  production duplicate `CURVATURE_DIM` (`geometry_ops.rs:9166-9170`) must change in lockstep
  and gains a drift-pin test equating it to `DimensionVector::CURVATURE` — today nothing
  catches divergence (`value_type_kind_matches` is kind-only).
- **δ signals.** Closure identities probed with rad⁻² MOI (scratch eval): ½Iω² → exactly
  ENERGY; τ/I → rad·s⁻²; L/t → exactly TORQUE. All capabilities delivered by the upstream
  #5844 (DAG-direction: wired dep) + this leaf's constant. No numeric bounds asserted —
  dimension-vector identities only. **Pin mechanism (D3-adjudicated):** typed param
  declarations, not stdout reading — `param rot_ke : Energy = ½·(1kg·1m·1m/(1rad·1rad))·
  (1rad/1s)²` checks exit 0 today (fixture `rotational_closure_prestate.ri`) and the same
  initializer declared `: Torque` exits 1 (non-vacuity control, probed 2026-08-10); the α
  probe harness's `ir` kind cannot evaluate stdout, so stdout-vector identities stay prose
  evidence only.
- **σ signals.** Field-population: the channel is a wrap-time linear projection of
  `fea.nodal_gradient` (stored at `elastic_static.rs:282`, projected at `:1127-1179`) — the
  same production path as curl; no `Undef` sentinel involved. Declared-codomain wrap
  verified (`sampled.rs:140-143,:297-305`). Angle-typed member access through a declared
  `.ri` field is the shipped #6164-model mechanics. **D3-verified additionally:** the channel
  declaration's own spelling `param shear_angles : Field<Point3<Length>, Vector3<Angle>>`
  parses AND resolves today (fixture `shear_angles_field_decl_pre.ri` — the Angle codomain in
  the `Field<D,C>` type-resolution arm was previously unexercised); the `fn f(v :
  Vector3<Angle>)` accept-probe is non-vacuous (a Length-component vector is rejected with a
  real overload diagnostic, fixture `shear_angles_vec3_wrongq_ctrl.ri`). **Gap bound:** no
  in-language `Vector3` component extraction exists (`.x` / `v[0]` / `norm(v) < 5deg` all
  reject) — component-value comparisons live in the Rust e2e pins (§9 B4, §12 Q6).
- **γ signals.** The crossing idiom is substrate-real (probed: `(s/r) * 1rad` → `2.5 rad`,
  `theta / 1rad` → dimensionless, `: Angle` annotation checks). Negative teaching (`1 rad`
  spaced form is a parse error) is probe-backed.
- **No guessed numeric bounds anywhere in this PRD** — every quantitative signal is either a
  dimension-vector identity or byte-identity of existing raw-f64 pins.

## §7 — Pre-conditions for activating

| Prerequisite | Why | Handling |
|---|---|---|
| #5844 (MOI → kg·m²/rad², pending) | δ's closure identities are false until it lands | hard `add_dependency` edge δ → 5844 |
| #6164 (`.rotation` channel, pending) | σ shares files (`solver_elastic.ri`, `compute_targets/*`) and the channel pattern; landing after avoids conflicts and lets σ's example sit beside `.rotation`'s pins | hard edge σ → 6164 |
| Registry τ8 (#6010) must not write the curvature row before α | else the row bakes m⁻¹ and α becomes a row migration | edge **6010 → α** wired at decompose (6010 is unprotected; the #6089→#6009 edges are precedent) |
| Grammar | no novel syntax — all fragments probe-parse today (`grammar_confirmed=true`); fixtures committed with this PRD | `tests/prd-gate/fixtures/angle_crossing_idiom.ri`, `curvature_rad_literal.ri` |

## §8 — Cross-PRD relationship + seam owners (G4)

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `builtin-signature-registry` τ8 (#6010) | produces | curvature/angle query rows cite α (basis `Ruling`) | registry PRD owns rows; **P owns the dimension** | edge 6010 → α |
| `builtin-signature-registry` τ7 (#6009) | none binding | field-op rows stay pure quotient, basis `Ruling("#6164")` — unchanged by P | registry PRD | already wired (#6164 cited in 6009's text) |
| `builtin-signature-registry` τ1 (#6003) | consumes | its ratified `x/1rad` diagnostic hints teach the crossing idiom; doctrine (β) is what they cite | τ1 owns hints; **P owns doctrine text** | no edge; prose cite |
| `differential-field-operators` (+#6164) | extends + amends | σ is an additive sibling channel of `.rotation`; α amends the PRD's "curvature precedent" wording (:17/D6) | **P** | σ → 6164 edge; docs amendment in α's diff |
| `dimension-checked-readers` (PRD 5) + #5844 | consumes | MOI re-dimension + reader gating (inertia readers, inverse-trig eval discipline) | PRD 5 / #5844 | δ → 5844 edge; no collision |
| `angle-units-surface-convergence` (PRD 3) | adjacent | PRD 3 owns the angle *surface* (bare-angle rejection, `angle_spec`, Torque/Nm, label alphabet) and its docs-truth slots ξ/ο/π/ρ (#5790/#5792/#5793/#5794) | PRD 3 | D9: content-disjoint; no edges; P must not touch `angle_spec`/bare-angle territory |
| `units-length-gate-completion` D11 (#5747) | ally | strict dimension equality is the enforcement direction P's doctrine cites | PRD 1 | no edge |
| #5825 (done, decision) | discharges | the `arc = r·θ` carve-out ("needs its own task") — the teaching half lands in β/γ | **P** | prose cite in β |
| **#6128 (rescoped) / #6131 (cancelled)** | supersession — RESOLVED 2026-08-10 | both predated #6164 and pulled curl toward `Vector3<Angle>`; Stage-2 reconciliation rescoped 6128 to implement #6164's `.rotation` channel (in-progress) and rewrote 6131 as resolved-by-6164; 6131 closed cancelled with Leo's authorization; #6164's text carries the do-not-double-land coordination note | — | resolved |
| GUI viewport (`MeshData.scalar_channels`) | produces | per-channel unit tag + signed range (υ) | **P** | consumer = #6164's channel; edge **6164 → υ (6185) WIRED** 2026-08-10 with Leo's authorization — the first signed Angle channel lands on the declared wire format |

Checked against the overlay's three known contested pairs: no overlap, no fourth instance.

## §9 — Boundary-test sketch (H; rows map to leaf signals)

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| B1 | Curvature is rad/m both ways | α | `param k : Curvature = 0.2rad/1mm` checks; `= 0.2/1mm` errors naming both dimensions; query-typing pins pass; display `rad·m^-1`; OCCT numeric pins byte-identical |
| B2 | Duplicate-const drift is caught | α | the new drift-pin test fails if `geometry_ops.rs`'s `CURVATURE_DIM` diverges from `DimensionVector::CURVATURE` (seeded-divergence self-check) |
| B3 | Rotational algebra closes end-to-end | δ, #5844 landed | CI example pins closure via **typed param declarations** — `param rot_ke : Energy = ½·I·ω²`, `param alpha : AngularAcceleration = τ/I` — so the declared-vs-initializer dimension-equality check makes `reify check` exit 0/1 the machine gate (D3-probed both polarities on the pre-state fixture: Energy → 0, Torque control → 1). The ω-vs-f 2π mistake is a compile diagnostic in the example's commentary fixture |
| B4 | Shear channel reads as Angle | σ, #6164 landed | Rust e2e pins (`differential_field_ops_e2e.rs`, same home as #6164's) read the sampled components harness-side and compare against deg-derived values; components = doubled symmetric off-diagonals × η (agrees with hand-computed ∇u on the patch fixture); the `.ri` example demonstrates sampling + typed passing to `fn f(v: Vector3<Angle>)` (accept-probe non-vacuous: wrong-quantity vector rejected, D3-probed); `.gradient` stays `Tensor<2,3,Real>`. **In-language component access on `Vector3` does not exist today** (`.x`, indexing, `norm`-compare all reject — D3 adversary probes), so no `.ri`-surface scalar comparison is asserted (§12 Q6) |
| B5 | Doctrine is walkable + discoverable | β, γ | INV-AD slugs resolve in `design-invariants.md` with checkable questions; an author who knows the *goal* ("turn this ratio into an angle", "why is torque N·m/rad") finds the idiom from `chunks/units.md` or the corpus index (transcripted acceptance) |
| B6 | Crossing idiom round-trips | γ | corpus example: `(s/r)*1rad` → Angle; `theta/1rad` → dimensionless; compiles in CI (`examples_smoke`) |
| B7 | Boundaries declare their angle convention | ι | STEP output carries an explicit radian PLANE_ANGLE declaration consistent with its payload; MCP `set_parameter` schema text states SI-radians; grep finds the OCCT/SolveSpace contract comments |
| B8 | GUI renders a signed dimensioned channel | υ | a signed test channel round-trips the IPC with its unit tag; the legend shows the unit; negative values survive range computation |

## §10 — Decomposition plan

- **α — Re-dimension CURVATURE to rad·m⁻¹** (core + pins + drift-pin + docs).
  *Modules:* `crates/reify-core`, `crates/reify-eval`, docs.
  *Signal:* B1 + B2 (fixtures: `curvature_rad_literal.ri` accepted, the dimensionless form
  rejected with the two-dimension diagnostic; drift-pin test present and self-checked).
  Includes: `dimension.rs:259` + doc rewrite (:248-258 re-adjudication of 3603),
  NAMED_DIMENSIONS ordering note (curvature = sole holder of rad·m⁻¹), literal pin flip
  (:1909-1931), `CURVATURE_DIM` duplicates (production `geometry_ops.rs:9166-9170` +
  test-local) + drift-pin, doc corrections (`kernel-geometry-queries.md:30`,
  `differential-field-operators.md` :17/D6 wording, `dimensioned-construction-strictness`
  manifest alias-count note). *Prereqs:* none. *Unlocks:* #6010 (edge wired).
- **β — Doctrine artifacts.** `docs/invariants.md` INV-DIM row(s) + `design-invariants.md`
  INV-AD-1..4 family (§4 content), crossing catalogue + closure identities + enforcement-
  status honesty (D7) + the #5825 arc-length teaching + the 2π/η distinction.
  *Signal:* B5 first half; files committed; slugs stable; G7 walk of *this batch* already
  cites them. *Prereqs:* none. *Unlocks:* γ.
- **γ — Docs-truth four-pack.** New crossing section in `chunks/units.md` (disjoint from
  #5790's slice), `examples/best_practices/angle_crossings.ri` + INDEX row, reify-design
  SKILL.md index line, discoverability transcript.
  *Signal:* B5 second half + B6. *Prereqs:* β.
  *Placement settled — do not re-open.* Relocating the crossing doctrine out of
  `chunks/units.md` into its own topic (lever a) and compressing its `Which ratio,
  though.` paragraph (lever b) were both costed and **declined** by #6290; see
  `docs/notes/angle-crossing-doctrine-placement-2026-08-19.md`.
- **δ — ANGULAR_ACCELERATION + rotational-closure exemplar.** New constant + NAMED_DIMENSIONS
  row + CI example (`examples/dynamics/rotational_closure.ri` or best_practices slot).
  *Signal:* B3. *Prereqs:* **#5844** (hard edge).
- **σ — `ElasticResult.shear_angles` channel + consumer example.** `.ri` decl beside
  `.rotation`, `sampled_shear_angles_field` beside `sampled_curl_field`, e2e pins beside
  #6164's, stdlib-reference entry, worked shear-limit example.
  *Signal:* B4. *Prereqs:* **#6164** (hard edge).
- **ι — Boundary declarations.** STEP PLANE_ANGLE declaration; OCCT `angle_rad` contract
  comment; SolveSpace `angle_deg` contract comment; MCP `set_parameter`/`get_parameters`
  schema honesty (inbound SI radians); dynamics `compliance_cell_f64` erasure comment; 2π
  marshalling-boundary comments (D4).
  *Signal:* B7. *Prereqs:* none.
- **υ — GUI channel bridge.** Unit/dimension tag on `scalar_channels` + legend + signed
  range handling.
  *Signal:* B8. *Prereqs:* none (recommend #6164 sequences after it — see §8).

Drift-guard registration check (overlay): α's drift-pin and σ's e2e pins live in existing
gate-resident suites (`dimension.rs` unit tests, `differential_field_ops_e2e.rs`) — no new
`tests/infra/test_*.sh`, no new wallclock bounds, no nextest partition changes → no
registration tasks needed. δ/γ examples ride existing corpus smoke gates.

## §11 — Out of scope (explicit)

- **Torsion** (empty referent — doctrine names it prospectively).
- **ANGULAR_MOMENTUM** constant — deferred bookmark; promoted when any producer/consumer
  materializes; algebra recorded in β.
- **`mean_curvature(sdf_field)` / Frenet named helpers** (H = η·∇²φ/2, κ = η·|dT/ds|) —
  greenfield; the laplacian path is test-only dimensionless today; deferred bookmark, the
  doctrine specifies their shape.
- **Generic `shear_angles` tensor builtin** (the `stress_invariants` shape) — no consumer
  today; bookmark.
- **Bare-angle/bare-dimensionless enforcement** — angle-units PRD (β/γ/δ/ε) and
  real-dimensionless-unification D5 territory (D7).
- **The STEP/3MF 1000× length mislabel** — filed as a standalone bug task at decompose
  (Length, not Angle; genuinely urgent interop defect).
- **`ScalarTorque.magnitude : Real` retype** — enshrinement-review Group C territory (C7),
  not angle-crossing work.
- **`Frequency` display ladder (`Hz`)** — display-unit-preference territory; noted.
- **Re-typing `curl`/`gradient`/`divergence` anywhere** — settled by #6164; #6128/#6131
  carry the superseded direction (§8 flag).
- **MOI implementation** — #5844's.

## §12 — Open questions (tactical)

1. **δ's example home**: `examples/dynamics/` vs `best_practices/` (INDEX contention with γ
   is file-lock-serialized either way). Decide at δ.
2. **σ's triple order/name**: Voigt (γ_yz, γ_zx, γ_xy) vs (γ_xy, γ_yz, γ_zx); channel name
   `shear_angles` vs `shear`. Default: Voigt + `shear_angles` (says what it carries). Decide
   at σ against #6164's landed naming.
3. **υ's tag shape**: unit string per channel vs full `DimensionVector` — default unit
   string (display-ready); decide against the GUI ladder plumbing at υ.
4. **ι's STEP fix interaction**: if the standalone 1000× bug task lands first, ι's STEP
   declaration reduces to verification. Coordinate, don't duplicate.
5. **`curvature` arg-aware Matrix overload** (`Matrix<2,2,Scalar<CURVATURE>>`,
   `units.rs:1422-1438`) tracks the constant symbolically — confirm no literal pin hides in
   the τ8 migration path. Check at α.
6. **`Vector3` component access does not exist in-language** (D3 adversary probes: `.x`
   "member access not yet supported", indexing "cannot index into non-collection type",
   `norm(v) < 5deg` "left operand must be a comparable kind" — and apparent resolution of
   reduction builtins like `magnitude` at the check surface is the known unknown-fn
   silent-accept, i.e. vacuous). σ's pins route component values through the Rust e2e
   harness instead. **This equally constrains #6164's `.rotation` signal** ("comparing
   against a deg literal") — its pins also live in `differential_field_ops_e2e.rs`, so the
   same routing applies; flagged in the chartering hand-back. If in-language component
   access lands later, promoting the comparisons into the `.ri` examples is a cheap
   follow-up.
