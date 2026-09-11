# PRD: Enum-shadow coherence — one module-wide meaning for a shadowing enum name, obligation-aware conformance diagnostics

Status: **active**. Authored 2026-08-20 in an interactive `/prd` session (design brief
`reify-5429-shadow-residuals`; four-agent investigation over branch `task/5429` and its
merge-base, findings re-derived empirically 2026-08-19/20). Shape: **B+H-lite** (contract §5
+ boundary-test sketch §6). Milestone: v0_6.

> **Code anchors** verified against main `128416067f` (2026-08-20), immediately after
> #5429's merge `bc8f74a4d4`. Cite-by-symbol is the default; re-locate at implementation
> time. Every "probe"-marked claim was measured 2026-08-19/20 with throwaway harness probes
> against branch tip `22b45744aa` (= the merged content) and merge-base `5df2f9f1f1`; the
> four committed `docs/prds/v0_6/fixtures/shadow_*.ri` fixtures re-observe the pre-state.

---

## §0 — Provenance & rulings

Task **#5429** (uniform-member-access ζ, decision D8) landed a `LocalEnumShadowScope`:
a module-local `enum N` outranks a prelude `structure def N` in bare declared-type
positions, so `enum Fit` + `param fit : Fit` lowers to `Type::Enum("Fit")` instead of
being conflated with `std.tolerancing`'s `structure def Fit`. Its reviewer verdict
(ISSUES_FOUND, 0 blocking; advanced past a wedged review-harvest gate per the steward
ruling) plus two earlier blocking-round findings form a residual bundle with **one root
cause**: the shadow set is a thread-local installed partway through the user module's
phase sequence, so it cannot reach types lowered **in a different compilation unit**
(the prelude's own compile) or **in an earlier phase** (`resolve_enum_variant_payloads`).

Leo's rulings (2026-08-20, this session):

1. **Severity at the obligation seam is Error** — a prelude trait obligation typed by a
   prelude structure is genuinely unsatisfiable by a same-named local enum; pretending
   otherwise defers the type error to eval.
2. **The omitted-member silent path is widened to the same diagnostic** — writing the
   member out and omitting it must agree.
3. **Standalone PRD with cross-amendments** to `uniform-member-access.md` and
   `stdlib-namespace.md` (landed in this PRD's commit), not a slice authored inside either.
4. **Value/ctor-position asymmetry is deferred but must stay tracked** (§7, D7).
5. **#5429 landed first** (merge `bc8f74a4d4`); this PRD's substrate is current main.

## §1 — Goal, consumers, user-observable surface (G1)

**Design principle (the whole PRD in one sentence):** *a shadowed name means the local
enum everywhere in the module; where a prelude obligation forces the prelude's meaning,
that is diagnosed at a site the user can act on, with provenance — never silently, and
never in type-constructor-leak language.*

| Consumer | What it consumes | Surface |
|---|---|---|
| **Every author** naming an enum with an ordinary engineering word | 127 prelude `structure def` names are claimable collisions (`Fit`, `Material`, `Color`, `Mode`, `Position`, `Datum`, `Layer`, `Part`, …); 7 of them carry prelude trait obligations (§2 table) | `reify check` diagnostics: coherent shadowing everywhere, one comprehensible Error where the collision is genuinely unsatisfiable |
| **#5391 standard-parts program** (bookmark) | pre-N3 collision survivability while the parts library multiplies user-declared names | its gate (4) neighbourhood |
| **stdlib-namespace α (#5493)** | `build_local_enum_shadow_set` + the shared entity-name filter helper (β here) as the fold-in seed for the NS-P1/P3 one-policy-point extraction | code seam (§7) |
| **uniform-member-access** fixture corpus | `member_enum_ctor.ri` (D8's headline) stays green — the regression floor | boundary row 8 |

Engine-integration sub-check (`engine-integration-norm.md` §3): compiler-side name
resolution + diagnostics only; no in-engine seam introduced — N/A.

## §2 — Background: the residual bundle *(probe)*

**R1 — prelude-trait conformance is an unintelligible hard error.** A prelude trait's
member type is lowered during the **prelude's own compile** (no user module exists, shadow
set empty) and frozen as `Type::StructureRef(N)`; the conforming user structure's member,
lowered inside the live shadow scope, is `Type::Enum(N)`; `implicitly_converts_to` has no
`Enum`/`StructureRef` arm → `TypeMismatchForTraitMember`. Because `StructureRef` renders
bare while `Enum` renders `Enum({name})` (`reify-core` `ty.rs` `Display`), the message
reads `expected Appearance, got Enum(Appearance)` — both sides print the same user-visible
name. At the merge-base this exact configuration was a **Warning**
(`TypeNotConformingToStructureRef`) — #5429 turned it into a hard error with a worse
message. Exposed surface (grep of all 47 stdlib modules, one member empirically confirmed
per shape):

| Prelude trait | Obligated member : type | Kind |
|---|---|---|
| `Visual` (std.materials.appearance) | `appearance : Appearance` | defaulted |
| `Physical` (std.structural, structural_physical.ri) | `material : Material` | **required** |
| `LocatedPort` (std.ports) | `frame : Frame3` | **required** |
| `ThreadedPort` (std.ports.mechanical) | `thread_spec : ThreadSpec` | **required** |
| `Input` (std.io) | `provenance : Provenance` | **required** |
| `SurfaceTreated` (std.surface_finish) | `coating : Coating` | defaulted |
| `SurfaceTreated` | `treatment : Treatment` | defaulted |

The four **required** members cannot be dodged by omission: conformance forces the user to
declare them, so `enum ThreadSpec` / `enum Material` + conformance is an unavoidable hard
error whose only remedy (rename the local enum) the diagnostic never states. (14 further
prelude trait members are typed by prelude *enums* — the mirror hazard if the rule ever
generalizes; out of scope, noted for α.)

**R2 — the omitted-member path silently keeps the conflation.** Omit the member and the
trait's own prelude-typed default fills the slot: 0 diagnostics, and the structure's member
is `StructureRef(N)` — the user's `enum N` is silently ignored. That is precisely the
silent conflation #5429 exists to eliminate, live on the inherited-member path. Root:
type-hygiene η (#4487) checks colliding members only (its design), and the checker's
omitted-member arm (`check_phase_check_members_against_requirements`, the collision loop's
`else { continue }`) skips straight to default injection. Net: `Appearance` means two
different things in one module depending on whether the user typed it out.

**R4 — the enum-variant payload axis.** `resolve_enum_variant_payloads` runs **before**
the `LocalEnumShadowScope` install in `compile_with_prelude_context_checked_with_config`
(the in-code comment even cites "has already run" as the placement's justification), and it
resolves payload field TypeExprs against `ctx.resolution_structure_names` — so a payload
field typed by a shadowed name lowers to `StructureRef(N)` while every param/let/fn/trait
position lowers to `Enum(N)`. Measured: passing the match binder of such a payload to a
same-typed fn param compiled **clean at the merge-base** and now fails overload resolution
(`no matching overload for use_fit(Fit), candidates: use_fit(Enum(Fit))`) — a silent→error
regression of exactly the cross-phase-disagreement class #5429 was built to kill.

**R3 — there is no de-shadowing escape hatch (routed out, not owned here).** Measured:
`import std.materials.appearance.Appearance as PreludeAppearance` → `unresolved type`
(identically **with no shadowing enum** — `ImportKind::EntityAliased` has zero
reify-compiler consumers; parser + LSP navigation only, acknowledged as stdlib-namespace
Smell 2b); `type PA = Appearance` → `unresolved type: PA` (pre-existing #6259: the alias
DFS hard-codes empty name sets and bypasses the shadow override's code path entirely);
`Base::Member` qualified types parse but resolve to `None` (deferred to
type-args-and-assoc-type-projection ιₑ). The sanctioned escape hatch is **qualified
references** (stdlib-namespace μ #5495 grammar + ν #5505 resolution). This PRD does
**not** wire entity-aliased imports (that would add a fourth naming mechanism the
namespace program deliberately did not plan) — see D6.

**Small items (from the #5429 review verdict).** S1: `build_local_enum_shadow_set` admits
**generic** enums, and the override collapses a bare generic-enum name to arity-erased
`Type::Enum(N)`, bypassing `resolve_enum_type_with_args`' arity/`Type::Applied` handling
(latent — no live stdlib generic collision; `Result<T,E>` is the generic prelude enum the
guard tests protect). S2: the test `applied_form_is_not_shadowed` asserts
`!matches!(result, Some(Type::Enum(_)))` where its documented contract is
`result.is_none()` — `Some(Type::Error)` would pass. S3: the
`seen_entity_names` structure/occurrence filter idiom now has a 4th copy
(`names_phase`, `entities_phase`, `defs_phase`, `enums_phase`); the new site **cannot**
reuse `ctx.resolution_structure_names` because that set also chains prelude template
names, whose subtraction would empty the shadow set for exactly the motivating cases.

## §3 — Resolved design decisions

- **D1 — Hoist the shadow scope to the whole resolving pipeline.** Move the
  `LocalEnumShadowScope` install to immediately after `pre_pass::collect_decl_refs`.
  Verified *(probe + code)*: both set inputs (`ctx.enum_defs`, `ctx.seen_entity_names`)
  are final there (sole writers in `pre_pass`; `resolve_enum_variant_payloads`'
  take/restore of `enum_defs` alters payloads, never names — and the build site is before
  that window); `phase_units` performs no type resolution at all; `phase_aliases` is
  doubly inert (hard-coded empty name sets, and `resolve_type_alias_expr` never enters
  `resolve_type_expr_with_aliases_kinded` where the override lives). The one resolving
  phase newly covered is `resolve_enum_variant_payloads` — which is the R4 fix.
  **Ratchet note:** after the hoist, a future #6259 fix that threads real name sets into
  the alias DFS will make alias bodies inherit shadow semantics — desired under NS-P3,
  but it must be #6259's explicit decision (note recorded on #6259 at decompose).
- **D2 — Obligation-aware conformance diagnostic, severity Error (Leo 2026-08-20).**
  Wherever conformance checking compares a prelude trait obligation whose declared type
  names `N` against a module whose shadow set contains `N`, emit one dedicated Error
  (new `DiagnosticCode`; contract §5 C2) instead of `TypeMismatchForTraitMember` /
  `TypeNotConformingToStructureRef`: primary label at the conformance site, secondary
  label at the user's `enum N` declaration, message naming the trait, the obligated
  member, the prelude module that declares `structure N`, and the remedy (rename the
  local enum; qualified references once #5505 lands). Feasibility verified: the scope is
  live throughout conformance (installed before `phase_entities`, dropped at end of the
  compile fn); the thread-local needs only a minted `pub(crate)` membership accessor.
- **D3 — Omitted = redeclared (Leo 2026-08-20).** The omitted-member arms (the
  requirements loop's `available_defaults` fallback and the collision loop's
  `else { continue }`) emit the **same** Error when the obligation's type names a
  shadowed `N`. This widens rejection from silent — deliberately: the silent path was
  silently *wrong* (the member bound the prelude structure while the module says enum),
  which is an INV-SF-3 `declared-intent-consumed-or-diagnosed` violation. Corpus blast
  radius today: zero (no tracked `.ri` collides with an obligation name).
- **D4 — No name-equivalence, no re-resolution, no shadow-set subtraction.** Rejected
  alternatives: (a) letting `Enum(N)` satisfy `StructureRef(N)` at the checker, or
  re-resolving the trait's retained `TypeExpr` under the user's scope — unsound; stdlib
  consumers of e.g. `Physical.material` need the structure's fields, so acceptance just
  defers the failure to eval with worse provenance (and requirements retain no source
  `TypeExpr` at all, so the general form needs a schema change for a semantics we don't
  want). (b) Subtracting the 7 obligation-carrying names from the shadow set — silently
  revives the pre-#5429 over-rejection wart on precisely the most collision-prone names
  and makes `N` mean two things again. (c) Forbidding the collision outright at the enum
  declaration — contradicts D8's headline (`enum Fit` must work) and jumps
  stdlib-namespace D-2's deliberately staged escalation.
- **D5 — No global `Display` change.** The `Enum({name})` leak is fixed *at this seam* by
  the D2 message's own wording (kind + provenance words; it never prints the two bare
  types against each other). `Type`'s `Display` is used everywhere and is not touched.
- **D6 — De-shadowing stays routed to stdlib-namespace.** `ImportKind::EntityAliased`
  stays unwired (their Smell 2b / D-7: qualification is via import binding names, μ+ν);
  `type`-alias bodies stay #6259's. The D2 diagnostic's remedy text carries "rename"
  today and is worded so ν's landing can extend it without reshaping the code.
- **D7 — Value/ctor-position asymmetry deferred, tracked (Leo 2026-08-20).** After this
  PRD, `N(...)` in ctor/value position still resolves the prelude structure while bare
  `N` in type position means the local enum. Tolerable short-run, not long-run. Tracking:
  **#5920** carries the explicit "should the type and value namespaces agree?" question;
  **stdlib-namespace α (#5493)** owns the NS-P3 kind-uniform policy point that resolves
  it. The §7 table row and the stdlib-namespace §6 amendment pin both so the scope cannot
  be silently dropped; the ω close-leaf re-verifies the pins exist before stamping.
- **D8 — Interim ownership + planned absorption.** This PRD is the **N0-interim owner**
  of the local-enum-vs-prelude-structure shadowing instance. stdlib-namespace α absorbs
  `build_local_enum_shadow_set` + the β helper into the NS-P1/P3 shared policy point;
  post-N3 (κ, D-2 stage 2) shadowing an *imported* stdlib name errors at the declaration,
  which makes the D2 diagnostic largely unreachable — that supersession is intended, and
  the impl sites carry absorption notes citing #5493/#5503 so α/κ fold rather than
  rediscover. The local-vs-local half (a same-module `structure def` still beats a
  same-module `enum`) is untouched here and durable (#5920's namespace question).

## §4 — Sketch of approach — mechanisms

- **M1 — Scope hoist (reify-compiler `lib.rs`)**: D1; rewrites the placement comment that
  currently justifies the pre-payload install; R4 fixture + harness tests pin payload
  positions lowering `Enum` for shadowed names.
- **M2 — Shadow-set hygiene (`enums_phase`, `type_resolution`)**: S1 — filter
  `.filter(|e| e.type_params.is_empty())` in `build_local_enum_shadow_set` + a test
  pinning that a bare generic-enum name is not collapsed; S2 — strengthen
  `applied_form_is_not_shadowed` case (a) to `assert!(result.is_none())`; S3 — extract a
  shared `CompilationCtx` helper for the local structure/occurrence name set (the
  local-only half; an explicit chained variant covers the three prelude-inclusive sites),
  documenting once why prelude template names must not be subtracted from the shadow set.
- **M3 — Obligation predicate substrate (`type_resolution`, `pre_pass`, conformance)**:
  a `pub(crate)` membership accessor for the shadow thread-local; enum-declaration spans
  recorded at `pre_pass` (the IR `EnumDef` carries no span — the AST does, exactly as
  `reserved_name_lint` consumes it) so the D2 secondary label can point at the enum;
  a name→declaring-prelude-module map derived from `prelude_refs` (the established
  `defs_phase`/`units_phase`/`prelude_context` idiom; foldable by α).
- **M4 — The diagnostic (`conformance/checker.rs`)**: one shared predicate + emission
  helper applied at the three sites — requirements-loop mismatch arm, requirements-loop
  `available_defaults` fallback, collision-loop (both the mismatch arm and the omitted
  `continue`); new `DiagnosticCode`; two labels; `with_candidates` may carry the rename
  suggestion. House template: `units_phase`'s "duplicate unit declaration — already
  defined in stdlib prelude" two-label diagnostic.
- **M5 — Docs & discoverability (project docs-truth gate)**: doc-chunk coverage
  (`crates/reify-mcp/src/tools/chunks/`) for prelude-name shadowing — the rule, the
  obligation collision, the remedy, and the obligation-carrying name list; `reify-design`
  cheatsheet index line; discoverability acceptance (an author who hit the collision by
  *goal* — "my enum name errors against a stdlib trait" — finds the rule and remedy).
  Exemplar-corpus (`examples/best_practices/`) deliberately N/A: this introduces no new
  authoring idiom, only a rule + diagnostic. This also closes the gap that
  uniform-member-access θ (#5431) carries no shadowing-docs deliverable.
- **M6 — Cross-PRD amendments + record corrections**: `uniform-member-access.md` D8/§7/§8ζ
  and `stdlib-namespace.md` §6 amended in this PRD's landing commit; at decompose:
  rewrite **#5969** (its fn-signature and local-trait-member surfaces are already fixed
  by the landed whole-sequence install; its skeleton-pass surface dissolves when
  stdlib-namespace γ #5494 deletes `build_structure_def_skeleton`; its imported-user-
  module-enums surface routes to α/NS-P3), add the D1 ratchet note to **#6259**, and file
  the type-param-defaults follow-up (§9).

## §5 — Contract (B+H-lite)

**C1 — Module-wide meaning.** For a module M whose shadow set contains `N` (module-local
non-generic `enum N`, no same-module `structure/occurrence N`): every declared-type
position lowered during M's compile through `resolve_type_expr_with_aliases_kinded` —
params, lets, fn signatures, local trait members, constraint defs, fields, **and
enum-variant payload fields** — resolves bare `N` to `Type::Enum(N)`. Known exclusions,
each with a named owner (not silent): `type`-alias bodies (#6259), ctor/value position
(#5920 / α, D7), type-param defaults (`convert_type_params`' unconditional
`StructureRef` fallback — follow-up **#6399**, §9), enums imported from other
user modules (α / the #5969 rewrite).

**C2 — Obligation diagnostic.** Fires iff conformance checking of a structure in M
evaluates a prelude-trait value-bearing obligation (requirement or default, param or let)
whose declared type names `N` ∈ M's shadow set. Payload: trait name; member name; `N`;
the prelude module declaring `structure N`; primary label on the conformance site;
secondary label on M's `enum N` declaration; remedy text. Severity **Error**, carries the
new `DiagnosticCode` (INV-SF-2/-6). It fires for redeclared **and** omitted members
(D3), replacing — never duplicating — the generic mismatch diagnostic at those sites: for
a given (structure, member) collision exactly **one** Error is emitted.

**C3 — Conservation.** Modules with no shadowed obligation name are byte-identical in
diagnostics and lowering: no shadowing → unchanged; shadowing without conformance to an
obligated trait → unchanged (the enum wins everywhere, no new diagnostic); conformance
without shadowing → unchanged (defaults inject silently as today). `member_enum_ctor.ri`
and the existing conformance/harness suites are the regression floor.

**C4 — Lifetime.** C2 is interim policy owned here until stdlib-namespace α/κ land; the
implementation keeps the policy surface small (one predicate, one emission helper, set
construction in one place) and cites #5493/#5503 at each site so absorption is a fold,
not an excavation.

## §6 — Boundary-test sketch (two-way; γ/α's observable signals)

| # | Scenario (fixture) | Pre | Post |
|---|---|---|---|
| 1 | redeclared defaulted obligation (`shadow_oblig_redeclared.ri`) | γ | exactly one Error, new code, message names `Visual` + std.materials.appearance; secondary label on `enum Appearance`; **not** `expected Appearance, got Enum(Appearance)` |
| 2 | omitted defaulted obligation (`shadow_oblig_omitted.ri`) | γ | the **same** Error as row 1 (today: 0 diagnostics, member silently `StructureRef`) |
| 3 | required obligation (`shadow_oblig_required.ri`, `Input.provenance`) | γ | the same Error via the requirements loop |
| 4 | shadowing, no obligated conformance (`enum Appearance`, no `: Visual`) | γ | clean; `param x : Appearance` lowers `Enum` (C3) |
| 5 | payload/binder coherence (`shadow_payload_binder.ri`) | α | `reify check` exits 0 (today: `no matching overload for use_fit(Fit)`); payload field lowers `Enum("Fit")` |
| 6 | renamed enum + prelude-typed conformance | γ | clean — the documented remedy works |
| 7 | generic local enum sharing a prelude structure name, bare type use | β | name **not** shadowed: resolves to the structure; applied form still reaches `resolve_enum_type_with_args` (S1/S2 pins) |
| 8 | `member_enum_ctor.ri` + existing conformance/cross-sub suites | α β γ | green, byte-identical diagnostics (C3 regression floor) |
| 9 | one-Error property | γ | rows 1–3 emit exactly one Error each — the generic mismatch diagnostic does not co-fire (C2) |

## §7 — Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `uniform-member-access.md` (parent, ζ = #5429 landed) | extends | D8's shadowing rule — its cross-unit/cross-phase residuals + the falsified "parallel-safe; type_resolution seam" premise | this PRD | D8/§7/§8ζ amendment lands in this PRD's commit |
| `stdlib-namespace.md` | feeds / superseded-by | α #5493 absorbs `build_local_enum_shadow_set` + β's helper into the NS-P1/P3 policy point; κ #5503 supersedes the D2 Error post-N3 (D-2 stage 2); ν #5505 owns the de-shadowing surface (NS-Q); D7's value-namespace question resolves under NS-P3 | per-item as listed | their §6 row lands in this PRD's commit |
| `type-hygiene.md` η (#4487, done) | refines | the collision rule's diagnostic at the shadowed-name intersection is replaced by C2; D3 adds the omitted-member check η's collision-only scope deliberately excluded; all non-shadowed behavior unchanged | this PRD | no edit to their PRD needed |
| **#6259** type-alias body gap | adjacent | eager alias DFS; D1's hoist means a future #6259 fix inherits shadow semantics in alias bodies | #6259 | ratchet note added to #6259 at decompose |
| **#5969** extend-#5429 follow-up | re-scoped | two of its four surfaces already fixed on main (whole-sequence install); skeleton-pass surface dissolves at stdlib-namespace γ #5494; imported-user-module enums → α | #5969 (rewritten at decompose) | rewritten 2026-08-20; now gated on #5493 |
| **#5920** enum entity-namespace gap | tracks | the type-vs-value namespace agreement question (D7); its local-vs-local precedence pin is preserved (as landed: `local_structure_wins_over_same_named_local_enum` in `enum_ctor_param_binding_tests.rs`; the planning-era name `same_module_structure_still_beats_same_module_enum` never landed — corrected on #5920 at decompose) | #5920 / α | verified its text carries the question |
| **#5391** standard-parts program | consumes | interim collision survivability for user part/material enum names | this PRD (interim), stdlib-namespace (end-state) | no new edge (its gates already ride stdlib-namespace) |

## §8 — Decomposition plan (Greek labels → real ids at decompose)

Serialized chain — all four code tasks touch `reify-compiler`'s resolution/conformance
neighbourhood; narrow file locks favor edges over contention. `metadata.files`
tight-or-empty. No novel grammar anywhere: `grammar_confirmed=true` batch-wide (fixtures
use existing syntax; validated against the checker 2026-08-20).

- **α — #6394 — Hoist the shadow scope over the whole resolving pipeline** (M1, D1). **LEAF.**
  Crates: reify-compiler. Signal: `shadow_payload_binder.ri` — `reify check` exits 0
  (today: overload Error, *probe*); harness test pins the payload field lowering
  `Enum("Fit")`; boundary rows 5, 8. Deps: none.
- **β — #6395 — Shadow-set hygiene: generic filter, assertion strength, shared filter
  helper** (M2). Intermediate → γ (γ's predicate consults the set β finalizes). Crates:
  reify-compiler. Signal: boundary row 7 pins (bare generic-enum name not collapsed;
  applied path preserved); the four filter-idiom sites collapse onto the shared helper
  (grep shows one definition). Deps: α (same-file adjacency; keeps locks serial).
- **γ — #6396 — Obligation-aware conformance diagnostics** (M3+M4, D2/D3).
  **LEAF — the headline.**
  Crates: reify-compiler, reify-core (`DiagnosticCode` variant). Signal: boundary rows
  1–4, 6, 9 — `shadow_oblig_{redeclared,omitted,required}.ri` each emit exactly one new
  Error naming trait + prelude module with the enum-declaration label; renamed-enum
  control clean; `member_enum_ctor.ri` green. Deps: β.
- **δ — #6397 — Docs: shadowing rule, obligation collision, remedy** (M5; docs-truth gate).
  **LEAF.** Modules: reify-mcp chunks, `.claude/skills/reify-design/SKILL.md` index.
  Signal: doc-chunk documents the rule + the 7 obligation names + the remedy, signatures
  registry-verified; cheatsheet index line present; discoverability — the collision
  findable from the chunks by intent query. Deps: γ.
- **ω — #6398 — PRD close** (overlay decompose-close obligation; filed at decompose). Signal: the
  committed terminal-status header (SHIPPED + leaf ids + AS-AUTHORED freeze + LIVE map,
  matching the data-carrying-enums shape) on this PRD and its capability manifest, after
  re-verifying the D7 tracking pins (§3). Deps: α, β, γ, δ.

DAG: α #6394 → β #6395 → γ #6396 → δ #6397 → ω #6398. All five filed 2026-08-20 in
one `planning_mode` batch; every edge is a real `add_dependency` edge (ω depends on all
four). Capability manifest: `enum-shadow-coherence.capability-manifest.md` + its
`.yaml` sidecar (33 bindings, all PASS).

**Companion corrections at decompose** (task-record writes, no code) — all four done
2026-08-20: **#5969** rewritten per §7 (re-scoped down to the imported-user-module-enums
surface alone and gated on stdlib-namespace α #5493; the other three surfaces are recorded
there as fixed-on-main or owned by #5494); the D1 ratchet note appended to **#6259**; the
type-param-defaults follow-up filed as **#6399** (gated on α #6394); and one correction
this session found — §7's #5920 row cited a planning-era pin name that never landed, fixed
in that row and on #5920 itself.

**G7 walk (reify INV-SF family, advisory here):** INV-SF-2 `error-severity-exits-nonzero`
— D2 is an ordinary Error (exit policy untouched); INV-SF-3
`declared-intent-consumed-or-diagnosed` — D3 exists precisely to close a violation;
INV-SF-6 `diagnostics-carry-codes` — C2 mints a code; no silent fail-soft added anywhere
(C3's conservation is byte-identical behavior, not a suppressed diagnostic); S3 removes a
duplication rather than adding one (the M3 provenance map is a 4th instance of an
established idiom, accepted with the C4 absorption note).

**Gate-test drift-guard:** all new tests land in the existing `harness_langcore` /
conformance integration-test binaries — no new gate-resident test binary, no new
`tests/infra` script → no registration obligations.

## §9 — Out of scope

- **De-shadowing syntax** (qualified refs, entity-aliased import wiring) →
  stdlib-namespace μ #5495 / ν #5505 (D6). Unblocking #5495 (work complete, verify-green,
  blocked on the DF reviewer-harvest defect #3639's class) is the shortest path to an
  escape hatch and is recommended, not owned, here.
- **Type-alias bodies resolving entity types** → #6259 (with the D1 ratchet note).
- **Value/ctor-position namespace agreement** → #5920 + stdlib-namespace α (D7 — deferred,
  tracked, never silently dropped).
- **Type-param defaults** — `convert_type_params` resolves defaults with an unconditional
  `StructureRef` fallback that bypasses both the shadow override and the enum fallback
  *(probe-adjacent, found by code reading)*; small, separable, filed at decompose as
  **#6399** (gated on α #6394) rather than widening C1 here.
- **General user-shadows-stdlib Warning→Error staging** → stdlib-namespace κ (D-2).
- **The enum-vs-enum shadow inversion and every other name kind** → stdlib-namespace α.
- **Enums imported from other user modules entering the shadow set** → α / the #5969
  rewrite.
- **Global `Type::Display` rendering change** (D5).
- **General ctor field-type conformance** → `struct-ctor-field-type-conformance.md`
  (undecomposed; D8's original referral stands).

## §10 — Open questions (tactical)

1. **Diagnostic code name + exact wording** (e.g. `ShadowedNameTraitObligation`). Follow
   the `units_phase` two-label template; decide during γ.
2. **`RequirementKind::Sub` obligations** — a sub requirement carries a bare structure
   name; if a shadowed `N` can reach it, extend the C2 predicate (cheap) or pin the
   existing failure shape. Check during γ.
3. **Span plumbing** — enum-decl spans via a small `CompilationCtx` map recorded in
   `pre_pass` vs. re-walking `parsed.declarations` at conformance time. Either is
   coherent; decide during γ.
4. **Provenance map placement** — build the name→module map at the D2 emission site vs.
   alongside the shadow-set construction in `lib.rs` (where `prelude_refs` is bound).
   Keep it foldable for α; decide during γ.
5. **Whether β's helper should also serve `names_phase`/`entities_phase`/`defs_phase`'s
   prelude-chained variant in one signature or two.** Decide during β.
