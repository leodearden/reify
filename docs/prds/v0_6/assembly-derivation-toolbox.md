# Assembly-derivation toolbox (Layers 1–2, Layer-3 contract)

**Status:** active — B+H contract PRD. Authored 2026-08-26 (assembly-derivation
design session: Leo as design authority; 6-agent groundwork team; full design
record in the session's design document, distilled here).
**Code anchors** verified against main `96041f850b` (2026-08-26). Main moves
fast — cite-by-symbol; re-locate lines at implementation time.

## 1. Goal

Ways to declare that one piece of assembly structure is DERIVED from another
instead of written out longhand — first-class, checkable, and honest about
chirality:

- **Layer 1 — derived sub instantiation**: `sub b = mirror of a across <plane>
  { … }` (reflective) and `sub b = image of a under <transform> { … }`
  (proper-rigid): a named instance that is the image of a sibling — prototype's
  definition re-instantiated with `prototype args ⊕ overrides`, per-child
  chirality dispositions (`keep`, `exclude`), derived placement (no `at`),
  fully addressable children.
- **Layer 2 — the image relation, placement axis**: `Relation` verbs
  `mirror_placed(a, b, plane)` / `image_placed(a, b, T)` in the existing
  `relate {}` machinery — CHECK direction in this PRD (both poses determined ⇒
  loud violation diagnostic when someone edits b independently); solve
  direction deferred (orientation-reversing solve is real solver work).
- **Layer 3 — symmetry values and orbits**: NOT built here. This PRD carries
  the binding compatibility contract (§8) so Layer 3 lands later without
  reworking Layers 1–2, plus a dep-gated [MILESTONE] capstone task that
  triggers the Layer-3 design+decompose session when the substrate exists.

User-observable on landing:
- The E4-shaped fixture (a CapstanUnit-like structure with a chiral helical
  child): `sub unit_b = mirror of unit_a across <yz-plane> { z_level = 55mm;
  keep drum }` renders the mirrored pulley web with the drum SAME-handed, via
  `reify build` STEP grep + GUI `mesh_stats`; `self.unit_b.<cell>` reads
  return image-frame (mirrored-local) values.
- The flip_ab-shaped fixture: `sub rail_l = image of rail_r under c2_z {
  span_au = 430mm; span_bu = default }` reproduces today's hand-written
  converse placement bit-exactly (`transform_compose` algebra), including the
  role-swapped override with `default` reset.
- `relate { image_placed(a, b, T) }` over a hand-built pair: satisfied when
  conforming; a loud violated diagnostic when b's placement is edited away.
- `rotation_about(point, dir, angle)` / `c2_about(point, dir)` build the
  conjugated axis-line rotation without hand-written `translate∘R∘translate⁻¹`.

## 2. Consumers (G1)

- **printer_v01 E4** (dogfood, the motivating consumer): unit-level mirror —
  the balanced side-swap with drums exempted (`keep`). The v3 forced
  duplicates (`FairleadPairMirrored`/`CapstanUnitMirrored`, dogfood branch
  804236401a) are deleted by PRD-0 #6592's fix via `side: -1`; THIS PRD's
  acceptance replaces the side-sign *mechanism itself* with `mirror of` on an
  un-instrumented prototype (see §5 — why the primitive still earns its keep).
- **The flip_ab / litter_tray idiom class** (`designs/litter_tray/
  bottom_deck_split.ri` C2 pair; printer `flip_ab` rails/idlers): every
  180°-converse pair written today as repeated args + a raw
  `orient_axis_angle` placement.
- **Hand-built symmetric pairs** (Layer 2): today's printer can assert
  `image_placed` over its translated units without adopting Layer 1 at all.
- **Layer 3** (future, this PRD's §8 contract + capstone): orbit instantiation
  consumes the derivation-map element IR and the builtins' names.
- In-engine seam conformance: derived subs ride the EXISTING realization &
  surfacing walks (engine-integration norm — same walks hand-written subs
  use; no new seam). The one new kernel-op use is `GeometryOp::Mirror`
  inserted in the surfacing walk (op-execute seam, already catalogued).

## 3. Foundations (design decisions, all ratified this session)

1. **Reflections never enter placements.** `Transform` stays proper-rigid (the
   spec's deliberate invariant; AP242 forbids det<0 in assembly placements;
   `axis2_placement_3d` cannot represent a left-handed frame; E1's dogfood
   commit independently verified "a reflection is not expressible as a
   placement"). Impropriety lives in the derivation-map element and LOWERS to:
   proper conjugated placements + one improper kernel op at the surfacing seam.
   `world = P_b ∘ M_loc ∘ (local content)`, with `P_b = M ∘ P_a ∘ M_loc⁻¹`
   proper by parity.
2. **Instantiate-then-map.** The image is a fresh instantiation of the
   prototype's definition with merged args (the PRD-0 machinery), followed —
   reflective form only — by a type-aware image map over instance cells:
   scalars identity; Vec3 positions mirrored; `Orientation` conjugated
   (M·R·M⁻¹, proper); child placements conjugated; leaf geometry reflected
   unless `keep`. Image cells are therefore **mapped local values** and the
   image's frame-to-parent stays a proper `Transform` — the ratified frame
   rule ("values carry their frame; reads compose through it") keeps working
   in SE(3). Isometry argument: metric predicates evaluated in the image's own
   evaluation hold; sign-sensitive predicates are re-evaluated, never assumed
   (re-evaluation is real because PRD-0 δ #6609 lands instance-scoped
   constraints).
3. **Chirality policy**: default reflect-all (always geometrically correct;
   achiral children come out identical); `keep <child>` is the explicit
   symmetry break (E4 drums) with default compensation `M_c` = the derivation
   plane conjugated into the child's local frame through the child origin
   (verified: E4's drum lands identity — same-handed translated pair);
   `keep … using <plane>` syntax RESERVED for v2 (CATIA-style declared
   equivalence plane); `exclude <child>` omits. `replace` deferred. v2:
   `chiral` entity annotation ⇒ un-dispositioned mirror of a chiral child is a
   hard error (new annotation, nothing breaks).
4. **Override reach**: param assignments + `<param> = default` reset (the
   flip_ab role-swap needs it) + local helper lets + image-added constraints
   (all specialization-body-legal); re-binding the prototype's lets REJECTED
   with `E_DERIVED_SUB_LET_OVERRIDE` → "promote `<let>` to a param" (Creo
   varied-items discipline; keeps Layer-2 checkability's "same definition"
   contract).
5. **No `at` on a derived sub** (`E_DERIVED_SUB_EXPLICIT_AT`): the placement IS
   derived. Verified against the real consumer: `c2_z ∘ transform3(id,
   vec3(rail_x,0,0))` equals rail_l's hand-written placement exactly.
6. **Verb naming is truth-scoped**: `mirror_placed`/`image_placed` check the
   placement axis only (Creo/CATIA two-axis split: placement-association ⊥
   geometry-association); `mirror_image`/`image_of` are RESERVED for future
   full-content verbs once #6583 (posed cross-sub kernel reads) enables
   geometry-level verification.
7. **Two-kernel story**: reflective lowering is OCCT-only in v1
   (`GeometryOp::Mirror` → `gp_Trsf::SetMirror` live; the Manifold adapter
   stubs mirror) — a Manifold-backed reflective derivation emits
   `E_DERIVED_SUB_KERNEL_UNSUPPORTED`, never silent wrong geometry.
8. **Export/BOM**: baked reflected B-reps + det=+1 placements are
   AP242-conformant by construction. Image instances carry `derived_from`
   provenance metadata (prototype path + map + dispositions) in snapshots;
   part-number identity (opposite-hand parts) is deliberately OUT — deferred
   to a future export/PDM PRD with the AP242 §7.7 `RelationType='mirrored'`
   mapping noted as its input. No derived-definition minting.

## 4. Pre-conditions (G3 substrate)

- **HARD: PRD 0** `docs/prds/v0_6/instantiation-value-flow.md` — merged-args
  instantiation driving the whole subtree (β #6592, after α #6586), loud
  failure (γ #6608), instance-scoped constraints (δ #6609). Without β, "image
  re-evaluated with overridden params" realizes template defaults — the exact
  #6592 disease. Real `add_dependency` edges wired at decompose.
- **Grammar**: the fourth `sub` arm does NOT parse today —
  `tests/prd-gate/fixtures/adt_mirror_of_arm.ri` (committed with this PRD)
  fails `tree-sitter parse` with an ERROR node, probe-verified 2026-08-26.
  Leaf A-α is the grammar producer; every syntax consumer depends on it.
- **Relation verbs parse today**: `tests/prd-gate/fixtures/
  adt_relation_verbs.ri` parses with 0 ERROR nodes (probe-verified) — relation
  verbs are ordinary calls; A-ζ needs vocabulary + check-mode work only
  (`relation_signatures.rs` + the `relate_solve` check path).
- Body-level mirror machinery exists on OCCT (`mirror()` 2-arg/7-arg,
  `affine_apply` det<0, `scale(solid, vec3(-1,1,1))`); `transform_compose` /
  `orient_compose` exist (bit-identical to operators). `Plane` values exist
  (stdlib `mirror(geometry, plane)` signature).
- Open kernel question (leaf A-ε, early): whether OCCT 7.8's det<0
  `BRepBuilderAPI_GTransform` output is consistently oriented — existing tests
  cannot observe it; system OCCT is 7.8 (reason from 7.8 headers, not
  reify-deps 7.9).

## 5. Why the primitive (vs post-PRD-0 side-sign parametrisation)

With #6592 fixed, `FairleadPair(side: -1)` becomes expressible — the manual
alternative. `mirror of` still earns its keep: side-sign is intrusive (the
prototype must be authored around the symmetry, every position threaded with
`side *` factors), sign-threading is the printer's recurring error class
(wrong-side idler; hand-kept ± mirrors), it has no chirality story, and it
yields two configurations of one definition rather than a named checkable
relation or an orbit. They compose: `side` stays right for interface choices
INSIDE a definition; `mirror of` handles structural images of un-instrumented
prototypes.

## 6. Contract (B+H)

- **D1 — Derivation-map element.** The IR payload on a derived
  `SubComponentDecl` is a *derivation map element* with constructors
  `mirror(plane-expr)` and `transform(expr)` — NOT a bare plane/transform.
  Layer 3's group elements become additional constructors of the same element;
  nothing downstream changes shape. (§8 contract item i.)
- **D2 — Placement algebra.** Proper form: `P_b = T ∘ P_a`. Reflective form:
  `P_b = M ∘ P_a ∘ M_loc⁻¹`, `M_loc` = the derivation plane conjugated into
  the prototype's local frame through the child origin. `at` on a derived sub
  is `E_DERIVED_SUB_EXPLICIT_AT`. Prototype must be a sibling sub with a
  DETERMINED placement (an `at auto` prototype is the joint-PRD seam with
  placement-relations-belt — rejected in v1 with a pointed diagnostic).
- **D3 — Image evaluation.** `merged_args = prototype's site args ⊕ overrides`
  (with `= default` restoring the definition default); evaluation =
  `elaborate_child_instance`-family instantiation (PRD-0 substrate), then the
  reflective image map (D4). Constraints of the image instance are checked
  against the image's effective valuation (PRD-0 δ).
- **D4 — Image map (reflective only).** Over the image's instance cells and
  realization: scalar → identity; Vec3 position → reflect through `M_loc`;
  `Orientation` → conjugate; child placement → conjugate (proper); leaf
  geometry → `GeometryOp::Mirror` in local frame, composed in the surfacing
  walk between the child handle and `ApplyTransform` (composed world transform
  stays proper). KNOWN HAZARD (documented, not solved): raw Vec3 lets meaning
  AXIAL quantities transform with an extra sign under reflection and the type
  system cannot tell them from positions — documented in θ's chunk; a
  `Direction`/`Axis` type distinction is out of scope.
- **D5 — Dispositions.** `keep <path>`: child subtree NOT reflected; placed at
  `M_loc·P_child·M_c` (M_c default per §3.3); cells unaffected (dispositions
  alter realized leaf geometry only). `exclude <path>`: subtree omitted from
  the image (cells too — reads of excluded members are unresolved-name
  errors). Paths must name members of the prototype's subtree
  (`E_DERIVED_SUB_UNKNOWN_DISPOSITION_PATH`).
- **D6 — Verbs.** `mirror_placed(a, b, plane)` / `image_placed(a, b, T)`
  return `Relation`; check mode (both determined): assert D2's equation to
  placement tolerance + same-definition sanity; violation = loud coded
  diagnostic. Solve mode = v2 (rejected with a diagnostic naming the
  deferral, not silently ignored — INV-SF-3).
- **D7 — Identity.** A derived sub is an ordinary named member: NodeId paths,
  entity paths, GUI tree, selectors, snapshots via the EXISTING walks (G7
  no-second-walk discipline); a `derived` badge + `derived_from` provenance
  metadata; no new naming scheme.
- **D8 — Diagnostics** (INV-SF-1/2/4/6 conformant): every new code
  Error/Warning carries a `DiagnosticCode`; failures ride the severity-exit
  gate; no silent undef (UndefCause on any realization-failure path);
  never-false-Violated preserved in Layer-2 check mode (a violated relation is
  VIOLATED — it is measurable; an unmeasurable one is Indeterminate with
  cause).

## 7. Boundary-test sketch

| # | Scenario | Precondition | Postcondition |
|---|---|---|---|
| T1 | `image of` reproduces flip_ab | flip_ab-shaped fixture | rail_l placement bit-exact vs hand-written (`transform_compose` equality) |
| T2 | role-swap override + `default` reset | same fixture | image has span_au=430, span_bu=definition default |
| T3 | `mirror of` mirrors the web | E4-shaped fixture | pulley positions x-negated in STEP/mock-op; constraints hold |
| T4 | `keep` exempts the chiral child | E4 fixture, `keep drum` | drum geometry same-handed (helix hand preserved), at mirrored station |
| T5 | image cells are mapped local values | E4 fixture | `self.unit_b.<vec3>` x-negated; scalars unchanged; frame stays proper Transform |
| T6 | overrides re-evaluate the image | E4 fixture `z_level = 55mm` | image geometry + cells reflect 55; prototype untouched |
| T7 | image constraints checked at effective valuation | override violating a prototype constraint | VIOLATED (rides PRD-0 δ) |
| T8 | `at` on derived sub rejected | bad fixture | `E_DERIVED_SUB_EXPLICIT_AT` observed |
| T9 | let-override rejected | bad fixture | `E_DERIVED_SUB_LET_OVERRIDE` + "promote to param" observed |
| T10 | cycle rejected | `a = image of b`, `b = image of a` | cycle diagnostic observed |
| T11 | `image_placed` check: conforming pair | hand-built pair fixture | satisfied |
| T12 | `image_placed` check: violated pair | edited pair | loud VIOLATED with code |
| T13 | `mirror_placed` on same-definition sanity | mismatched defs | diagnostic observed |
| T14 | Manifold reflective derivation | manifold kernel | `E_DERIVED_SUB_KERNEL_UNSUPPORTED` observed, no silent geometry |
| T15 | `rotation_about` axis-line conjugation | builtin fixture | placement equals hand-conjugated composition |
| T16 | GUI addressability | E4 fixture | image children selectable; badge shown; inspector lists overrides+dispositions |
| T17 | OCCT det<0 orientation | A-ε probe | mirrored solid valid, orientation consistent (or finding documented + upstream issue filed) |
| T18 | derived sub of derived sub | chained fixture | composes or is cleanly rejected (decide at A-β; either way LOUD) |

## 8. Layer-3 outline (design-for now, build later — the capstone's input)

Grounding (formalisms survey, session record): glob→regex = enumerated copies →
finitely generated GROUP ACTING on geometry. Orbit = the regex's language;
orbit–stabilizer = counts without enumeration; Coxeter theory decides mirror-set
closure from angles alone; crystallography (Wyckoff positions, exact symbolic
operators) is the shipped precedent; PGA supplies the type discipline (planes +
motions one algebra, parity bit; conjugation = one operation; quat+translation
IS the even subalgebra). Surface words stay mirror/rotate — users never see the
DFA.

Sketch (names provisional):
```reify
symmetry hub_sym = generated {
    m : mirror(xz_plane)
    r : rotation(z_axis, 72deg)   // compiler closes: order 20; non-closing
}                                  // angles rejected ("nearest closing: 45°")
sub bolts[g in hub_sym] = Bolt() at g * seat_pose   // orbit instantiation
```
Key semantics: symbolic group elements (words in generators; floats only at
leaves); stabilizers declared-and-checked (a prototype nudged off its mirror
plane fails at the edit site, never a silent count jump); instance addressing
by element (`bolts[m*r^2]`); hierarchy = placing a sub conjugates its symmetry
declarations (wreath composition falls out; property tests). Infinite
translation lattices stay out (indexed subs / linear arrays own them).

**The binding compatibility contract (Layers 1–2 MUST honor now):**
i. The derivation-map element (D1) admits group-element constructors later —
   plane/transform are constructors, not the payload type.
ii. `symmetry` is reserved as a contextual keyword by leaf A-α (grammar
   comment + spec keyword-note; no grammar built for it).
iii. Element addressing `xs[<element>]` is documented beside the positional
   NodeId scheme as reserved (no implementation).
iv. Orbit instantiation generalizes the **indexed-sub domain clause** — the
   extension point `indexed-sub-instantiation.md` §8 explicitly reserves
   ("a future PRD may generalize the domain clause"). Recorded here, in THIS
   PRD's seam table — the indexed-sub PRD is not edited unilaterally.
v. `rotation_about`/`c2_about` (A-η) are named so Layer-3 element constructors
   adopt them unchanged.

**The capstone (leaf A-μ, [MILESTONE], execution_class=decision):** when its
deps land (A-κ integration gate; #5482 β + #5483 γ indexed-sub substrate;
PRD-0 β #6592), it escalates to Leo to run the Layer-3 `/prd` design+decompose
session, with this §8 + the session's formalisms survey as inputs.

## 9. Cross-PRD relationships (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| instantiation-value-flow.md (PRD 0) | consumes | merged-args elaboration (β #6592 after α #6586); loud failure (γ #6608); instance constraints (δ #6609) | PRD 0 | wired (hard dep edges) |
| indexed-sub-instantiation.md | future Layer-3 seam | domain-clause generalization (its §8 extension point) | THIS PRD records the intent; the capstone session negotiates the joint shape; no unilateral edit of theirs | capstone-gated |
| placement-relations-belt.md / geometric-relations.md | boundary + vocabulary | `at auto` prototypes REJECTED v1 (their seam); Layer-2 verbs enter the relation vocabulary (`relation_signatures.rs`) in check mode only | solver-placed derived instances = joint follow-up per the standing two-PRD rule; check-mode verbs = this PRD (A-ζ) | declared |
| uniform-member-access.md (#5426) | sibling substrate | let-instance realization — unaffected; derived subs are schema members, not values | that PRD | reference |
| #6583 | blocks Layer-2 v2 only | posed cross-sub kernel reads for geometry-level verification | that task | reserved verbs (`mirror_image`) wait on it |
| sub-placement-and-surfacing.md | consumes/extends | the surfacing walk hosts the improper-op insertion (op-execute seam) | this PRD (A-δ) | queued |
| Future export/PDM PRD | produces input | `derived_from` provenance + AP242 §7.7 'mirrored' mapping note | that future PRD owns part numbers | out of scope here |

## 10. Decomposition plan (task IDs assigned at decompose, 2026-08-26)

- **A-α = #6615 — grammar producer** [high]: fourth `sub` arm (`mirror of <ident>
  across <expr> { … }` / `image of <ident> under <expr> { … }`; block items:
  param assignment incl. `= default`, `keep <path>` (reserving `using
  <plane>`), `exclude <path>`, local `let`, `constraint`) in
  tree-sitter + the GUI lezer mirror + reify-syntax lowering into AST
  (`SubDecl` derivation fields); `symmetry` contextual-keyword reservation
  note; spec §4.7 + §15 amendment. Signal: the committed
  `adt_mirror_of_arm.ri` fixture parses with 0 ERROR nodes (today: ERROR,
  probe-verified). INV-SF-7: ambiguity-regression corpus rows for the new arm
  vs quantity-literal juxtaposition. (The `at`-rejection diagnostic T8 is
  A-β's alone — the grammar may accept `at` syntactically so the compiler can
  reject it with a better message; D3-adversary contested-ownership finding,
  resolved to β.)
- **A-β = #6616 — compiler lowering** [high, deps A-α]: derivation spec on
  `SubComponentDecl` (D1 element; prototype ref; overrides; dispositions);
  compile-scope validation (unknown prototype / non-sibling / cycle /
  disposition path / let-override / explicit-at / auto-prototype), each a
  coded diagnostic observed by a fixture. T8–T10, T18 decision.
- **A-γ = #6617 — eval value plane** [high, deps A-β, PRD-0 α#6586 + β#6592]:
  placement algebra (D2), merged-args instantiation, image map over instance
  cells (D4 value half). Signals: T1, T2, T5, T6 (values via CLI eval/build).
- **A-δ = #6618 — geometry plane + dispositions** [high, deps A-γ]: improper-op
  insertion in the surfacing walk; `keep`/`exclude` (D5); Manifold rejection
  (T14); provenance metadata (D7). Signals: T3, T4, T14 via STEP grep +
  mock-op recording.
- **A-ε = #6619 — OCCT det<0 orientation probe** [medium, independent, EARLY]:
  OCCT-gated test asserting mirrored-solid validity/orientation consistency
  (`BRepBuilderAPI_GTransform` and `SetMirror` outputs; STEP writer behavior
  on reflected B-reps). Signal: T17 test in the gate (or a documented finding
  + follow-up task if OCCT misbehaves).
- **A-ζ = #6620 — Layer-2 check verbs** [medium, deps A-γ]: `mirror_placed` /
  `image_placed` in the relation vocabulary; check mode; solve-mode deferral
  diagnostic (INV-SF-3). Signals: T11–T13 (verbs parse today —
  `adt_relation_verbs.ri` committed, probe-verified).
- **A-η = #6621 — rotation builtins** [medium, independent]: `rotation_about(point,
  dir, angle)`, `c2_about(point, dir)` → `Transform<3>` (registry + eval +
  stdlib-reference). Signal: T15.
- **A-θ = #6622 — GUI derived badge + inspector** [medium, deps A-δ]: badge,
  prototype link, overrides/dispositions in inspector; tree addressability.
  Signal: T16 via debug MCP.
- **A-ι = #6623 — docs-truth bundle** [medium, deps A-δ A-ζ A-η]: chunk (derivation
  primitives incl. the axial-vec3 hazard + chirality policy), best_practices
  exemplar (litter_tray C2 pair via `image of`; a `mirror of` + `keep`
  example) + INDEX.md, reify-design cheatsheet line, intent-level
  discoverability ("make the left-hand version", "mirror this subassembly").
- **A-κ = #6624 — integration gate** [medium, deps A-δ A-ζ A-η A-θ]: E4-shaped +
  flip_ab-shaped fixtures committed & registered; CLI STEP-grep + mesh_stats
  runs; the §1 bullets executed. Signal: the canonical user-observable run.
- **A-λ = #6625 — PRD close** [low, deps all; docs, normal/simple]: ID backfill,
  terminal stamp, AS-AUTHORED freeze, manifest header.
- **A-μ = #6626 — Layer-3 capstone** [MILESTONE, execution_class=decision, deps A-κ +
  #5482 + #5483 + PRD-0 β#6592]: escalates to Leo to run the Layer-3 /prd
  design+decompose session with §8 as input. Signal: the escalation fires
  exactly when the substrate lands (dep-gating observable in the task store).
  **Filed as a STANDALONE milestone task, not a PRD-A decomposition leaf**: it
  deliberately outlives this PRD's shipping (A-λ's close stamp names it as the
  live continuation; making it a leaf would block SHIPPED on Layer-3 work).

## 11. Out of scope

- Layer-3 implementation (symmetry values, orbits, stabilizers) — capstone.
- Solve-direction `mirror_placed` (orientation-reversing relate solve);
  `at auto` prototypes; solver-placed derived instances (joint PRD seam).
- Geometry-level relation verification (`mirror_image` verbs) — blocked on
  #6583, names reserved.
- `chiral` annotations; `keep … using <plane>` validation; `replace`
  disposition — v2, syntax space reserved.
- Part-number/BOM identity of opposite-hand parts; derived-definition minting.
- Manifold mirror support (rejection diagnostic only in v1).
- General affine placements; any change to `Transform` rigidity or AffineMap
  one-way-ness; automatic symmetry detection; un-fusing pattern builtins
  (indexed subs own addressable repetition).
- Editing `indexed-sub-instantiation.md` (the domain-clause seam is recorded
  here; the capstone session negotiates the joint shape).

## 12. Open questions (tactical)

1. Final diagnostic code names (`E_DERIVED_SUB_*` family; INV-SF-6 codes
   mandatory).
2. T18 (derived-of-derived): compose vs reject-in-v1 — decide at A-β; loud
   either way.
3. Placement-tolerance for the Layer-2 check (exact vs epsilon; consult the
   relate solver's existing tolerances) — decide at A-ζ.
4. Whether the E4-shaped fixture also lands as an `examples/best_practices/`
   exemplar or stays prd-gate-only — decide at A-ι/A-κ.
5. Exact `M_c` spelling in the spec prose (the conjugated-plane default) —
   wording at A-α's spec amendment.
