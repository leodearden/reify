# Capability manifest — `enum-shadow-coherence.md`

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/enum-shadow-coherence.md`.
Built at decompose, 2026-08-20. Machine-readable twin:
`docs/prds/v0_6/enum-shadow-coherence.capability-manifest.yaml`.

> **Code anchors** verified against main `4a1f082ed0` (2026-08-20). Cite-by-symbol
> throughout — no `path:line` anchors (overlay rule *Code anchors in PRD prose*).
> Re-locate at implementation time.

## Staleness control for the PRD's `(probe)` measurements

Every `(probe)`-marked claim in the PRD was measured 2026-08-19/20 against branch
tip `22b45744aa` (= the content merged as `bc8f74a4d4`). Re-checked at decompose:
`git diff --name-only bc8f74a4d4..HEAD -- crates/` lists **16** files, and **none**
is in the resolution/conformance neighbourhood — `type_resolution.rs`, `lib.rs`,
`compile_builder/*` and `conformance/*` are untouched since the merge. The changed
set is stdlib `units.ri` (additive `Pressure2`/`Pressure3` aliases), dynamics/FEA
stdlib + eval, OCCT kernel, and their tests. The probe measurements therefore still
describe current main, and no leaf's RED baseline needs re-measuring before dispatch.

No `reify check` re-run was performed at decompose: the host was CPU-saturated
(load ≈128 on 32 cores) and both checked-in binaries (`target/{debug,release}/reify`)
predate the `bc8f74a4d4` merge, so a re-run would have required a cold rebuild that
would starve the running orchestrator. The staleness argument above is the
substitute, and it is a stronger one than a rebuild would have been for the
`type_resolution`/`conformance` surface.

## Batch-wide bindings

| Check | Evidence | Verdict |
|---|---|---|
| Grammar reality (all 5 fixtures) | `tree-sitter parse --quiet` from `tree-sitter-reify/`, 2026-08-20: `shadow_oblig_redeclared.ri`, `shadow_oblig_omitted.ri`, `shadow_oblig_required.ri`, `shadow_payload_binder.ri`, `member_enum_ctor.ri` — all exit 0, 0 ERROR nodes | **PASS** |
| No novel grammar | The PRD introduces no syntax; `grammar_confirmed=true` batch-wide | **PASS** |
| Fixture inertness | `docs/prds/v0_6/fixtures/*.ri` are read only by explicit `include_str!` from named tests (`enum_ctor_param_binding_tests.rs`, `uniform_member_path_tests.rs`) — no glob sweep — so the three committed `shadow_oblig_*.ri` fixtures (which produce hard Errors on today's main) cannot redden the gate before α/γ wire them | **PASS** |
| G7 (reify INV-SF/INV-AD families) | No hit, no waiver. INV-SF-2 `error-severity-exits-nonzero`: D2 is an ordinary Error. INV-SF-3 `declared-intent-consumed-or-diagnosed`: D3 exists to close a violation. INV-SF-6 `diagnostics-carry-codes`: C2 mints a code. INV-SF-1/-4/-5/-7 and INV-AD-1..4: N/A (no `Undef`, no Indeterminate, no placeholder-typed signature, no parser change, no angle/dimension surface) | **PASS** |
| Gate-test drift-guard | All new tests land in the existing `harness_langcore` / conformance integration-test binaries; no new `tests/infra/test_*.sh`, no new gate-resident binary, no new wall-clock assertion → no registration obligation, so no ordering hazard of the esc-4914-162 shape | **PASS** |

---

## α — Hoist the shadow scope over the whole resolving pipeline

| # | Capability | Binding | Verdict |
|---|---|---|---|
| α1 | shadow-scope substrate exists and is wired on main | `LocalEnumShadowScope` (RAII guard over the `RESOLUTION_SHADOWING_ENUM_NAMES` thread-local, `type_resolution.rs`) + `build_local_enum_shadow_set` (`compile_builder/enums_phase.rs`), installed at exactly one production site inside `compile_with_prelude_context_checked_with_config` (`reify-compiler/src/lib.rs`) — not test-only | **PASS** |
| α2 | both set inputs are final at the hoist target | `build_local_enum_shadow_set` reads `ctx.enum_defs` + `ctx.seen_entity_names`; its own doc comment states both are final from `pre_pass::collect_decl_refs` onward. The hoist target is the statement immediately after the `collect_decl_refs` call | **PASS** |
| α3 | **payload resolution reaches the shadow override** (G6 branch 3 — the load-bearing one) | `resolve_enum_variant_payloads` resolves each named payload field via `resolve_type_expr_with_aliases`, which is a thin delegator to `resolve_type_expr_with_aliases_kinded` — the function that hosts the shadow override. So moving the install earlier genuinely reaches R4; it is not a no-op | **PASS** |
| α4 | the newly-covered phases are inert or intended | Between `collect_decl_refs` and today's install site: `phase_units` (no type resolution), `phase_aliases` (doubly inert — hard-coded empty name sets, and `resolve_type_alias_expr` never enters the `_kinded` resolver), `build_resolution_names` (name-set construction only), `resolve_enum_variant_payloads` (**the R4 fix**), `build_resolution_enums_from_cache` | **PASS** |
| α5 | RED baseline is real | `shadow_payload_binder.ri` fails today with `no matching overload for use_fit(Fit), candidates: use_fit(Enum(Fit))` (probe 2026-08-19/20; staleness re-checked above). Merge-base was clean — a genuine silent→error regression, not a pre-existing failure | **PASS** |
| α6 | grammar-fixture | `grammar-fixture:docs/prds/v0_6/fixtures/shadow_payload_binder.ri` parses, 0 ERROR nodes (2026-08-20) | **PASS** |

## β — Shadow-set hygiene (S1 / S2 / S3)

| # | Capability | Binding | Verdict |
|---|---|---|---|
| β1 | S1 gap is real and fixable in place | `build_local_enum_shadow_set` collects `ctx.enum_defs` names filtered only against local structure/occurrence names — no `type_params.is_empty()` filter. `EnumDef` carries `type_params` (already populated by `collect_decl_refs`), so the filter is a one-predicate addition | **PASS** |
| β2 | S1 is latent, not live | Measured 2026-08-20: `Result` is the **only** generic enum in the stdlib prelude, and the intersection of stdlib enum names with the 127 stdlib `structure def` names is **empty**. So no live stdlib collision exists; the hazard is a *user's* generic enum colliding with a prelude structure name. The S1/S2 guard tests protect `Result<T,E>` | **PASS** |
| β3 | S2 assertion weakness is real | `applied_form_is_not_shadowed` case (a) asserts `!matches!(result, Some(Type::Enum(_)))` while its own doc comment states the contract as "must stay unresolved here" — `Some(Type::Error)` passes today | **PASS** |
| β4 | S3 duplication count is exact | The `*kind == "structure" \|\| *kind == "occurrence"` filter over `ctx.seen_entity_names` appears at exactly **four** sites: `compile_builder/{defs_phase, entities_phase, enums_phase, names_phase}.rs` (measured 2026-08-20). `CompilationCtx` (`compile_builder/ctx.rs`) is the natural helper home | **PASS** |
| β5 | S3 must not reuse `ctx.resolution_structure_names` | That set chains prelude template names, so subtracting it would empty the shadow set for exactly the motivating cases (`Fit`, `Appearance`, …). β's helper is the **local-only** half; the three prelude-inclusive sites need an explicit chained variant (OQ#5) | **PASS** |
| β6 | downstream consumer named (G2 intermediate) | γ — γ's C2 predicate consults the set β finalizes; boundary row 7's pins are β's own observable | **PASS** |

## γ — Obligation-aware conformance diagnostics (the headline)

| # | Capability | Binding | Verdict |
|---|---|---|---|
| γ1 | the §2 obligation table is exact | Verified 2026-08-20 against the stdlib: `Visual.appearance : Appearance` (defaulted, `= Appearance()`), `Physical.material : Material` (required), `LocatedPort.frame : Frame3` (required), `ThreadedPort.thread_spec : ThreadSpec` (required), `Input.provenance : Provenance` (required), `SurfaceTreated.{coating : Coating, treatment : Treatment}` (both defaulted). All 7 obligated type names are among the **127** prelude `structure def` names (count re-measured, matches the PRD) | **PASS** |
| γ2 | the scope is live throughout conformance | The RAII guard is installed before `phase_functions`/`phase_traits`/`phase_entities` and drops at the end of `compile_with_prelude_context_checked_with_config`; the conformance chain `phase_entities → compile_entity → check_trait_conformance → check_phase_check_members_against_requirements` runs entirely inside it | **PASS** |
| γ3 | a membership accessor is mintable | `RESOLUTION_SHADOWING_ENUM_NAMES` is a `thread_local!` `RefCell<HashSet<String>>` in `type_resolution.rs` with a `LocalEnumShadowScope` guard and **no** reader outside the override. Adding a `pub(crate)` membership fn is purely additive | **PASS** |
| γ4 | the emission sites are exactly locatable | `DiagnosticCode::TypeMismatchForTraitMember` is emitted at **four** sites in `conformance/checker.rs`. M4's targets are three of them plus the two silent arms: the requirements-loop explicit-member mismatch arm; the requirements-loop `available_defaults` wrong-type arm **and** its silent same-kind-matching-default arm (the R2 omitted path); the collision loop's mismatch arm **and** its `let Some(conformer_type) = … else { continue }` (the second R2 omitted path). The fourth emission site — trait-let annotation drift inside the default-injection phase — is **out of γ's scope** | **PASS** |
| γ5 | enum-declaration spans are available | `reify_ast::EnumDecl` carries `span: SourceSpan`; `reserved_name_lint` already consumes it as `Declaration::Enum(e) => (…, e.span)` and is invoked with `parsed` from the same compile fn. The lint's own doc note records that these decls expose no `name_span`, so D2's secondary label points at the whole `enum N` declaration (OQ#3 is a placement question, not a feasibility one) | **PASS** |
| γ6 | the `Enum({name})` leak is real and D5-scoped | `Type`'s `Display` renders `Type::Enum(name)` as `Enum({name})` and `Type::StructureRef(name)` bare (`reify-core/src/ty.rs`), producing `expected Appearance, got Enum(Appearance)`. D5 fixes it *at this seam* via D2's wording; `Display` itself is untouched | **PASS** |
| γ7 | house template exists | `units_phase`'s `"duplicate unit declaration '{}' — already defined in stdlib prelude"` two-label diagnostic (`with_code` + `with_label`) is the idiom M4 follows; `DiagnosticCode` is a `pub enum` in `reify-core/src/diagnostics.rs`, so minting a variant is additive (INV-SF-6) | **PASS** |
| γ8 | rejection-mechanism, pre-state observed (G6 branch 4) | Rows 1/3 (`shadow_oblig_redeclared.ri`, `shadow_oblig_required.ri`): the pre-state rejection **is observed to fire** today — hard `TypeMismatchForTraitMember`. Row 2 (`shadow_oblig_omitted.ri`): the rejection is observed **ABSENT** — 0 diagnostics, member silently `StructureRef` — which is precisely the R2 defect D3 closes, recorded here as the defect rather than as motivation. Both measured 2026-08-19/20 and pinned in the fixture headers. γ's own post-state Error is γ's deliverable, so the capability's producer is γ itself, not a downstream task | **PASS** |
| γ9 | D3's corpus blast radius | Measured 2026-08-20 over all 631 tracked `.ri`: the **only** files declaring an enum whose name is an obligation type are this PRD's own three `shadow_oblig_*.ri` fixtures. No existing tracked design collides, so D3's widening breaks nothing | **PASS** |
| γ10 | grammar-fixtures | All three `shadow_oblig_*.ri` parse with 0 ERROR nodes (2026-08-20) | **PASS** |
| γ11 | C3 regression floor is reachable | `member_enum_ctor.ri` is already wired into `harness_langcore/enum_ctor_param_binding_tests.rs` via `include_str!`, alongside #5429's conformance/overload oracles — the floor is an existing green suite, not one γ must build | **PASS** |

## δ — Docs: shadowing rule, obligation collision, remedy

| # | Capability | Binding | Verdict |
|---|---|---|---|
| δ1 | the doc gap is real | `crates/reify-mcp/src/tools/chunks/` holds 17 chunks and **zero** of them contain the string "shadow" (case-insensitive, measured 2026-08-20). The rule, the obligation collision and the remedy are undocumented across the whole chunk corpus | **PASS** |
| δ2 | the cheatsheet index target exists and is likewise empty | `.claude/skills/reify-design/SKILL.md` exists; **zero** "shadow" hits (2026-08-20) | **PASS** |
| δ3 | signature-verification obligation is vacuous here, deliberately | The docs-truth gate's "every documented signature verified against the compiler arms/registries" clause has no subject: this PRD documents a *name-resolution rule and a diagnostic*, not builtins or stdlib signatures. Recorded so the δ implementer does not manufacture a check — not skipped silently | **PASS** |
| δ4 | exemplar-corpus deliverable is N/A, with reason | M5 states it explicitly: no new authoring idiom is introduced, only a rule + diagnostic, so `examples/best_practices/` and its `INDEX.md` are untouched. The other three docs-truth deliverables (chunk, cheatsheet index, discoverability acceptance) are all in δ's scope | **PASS** |
| δ5 | δ also closes an inherited gap | uniform-member-access θ (#5431) carries no shadowing-docs deliverable; δ is where D8's rule finally gets documented | **PASS** |

## ω — PRD close (overlay decompose-close obligation)

| # | Capability | Binding | Verdict |
|---|---|---|---|
| ω1 | the terminal vocabulary and freeze-header shape are fixed | The overlay's closed three-value set (`SHIPPED` / `SUPERSEDED` / `WITHDRAWN`) plus the ratified three-part freeze header, exemplified by the headers of `docs/prds/v0_6/data-carrying-enums.md` and `docs/prds/kernel-seam-contracts.md`. ω copies that shape, not #4438's or #3847's non-conformant output | **PASS** |
| ω2 | the D7 tracking pins exist and are live | #5920 (the type-vs-value namespace question) and #5493 (stdlib-namespace α, the NS-P3 policy point) are both live non-terminal tasks as of 2026-08-20. The §7 rows and the `stdlib-namespace.md` §6 amendment landed in the PRD's own commit `4a1f082ed0`. Per the overlay, ω cites the **IDs**, never their status | **PASS** |
| ω3 | leaf IDs are backfilled before the stamp | Decompose-close obligation (1) — this session backfills the real IDs into §8's leaf rows and commits them beside the stamped sidecar. ω re-verifies the rows carry IDs before stamping | **PASS** |
| ω4 | cancelled-dependency disposition | ω depends on α/β/γ/δ by real `add_dependency` edges. Per the overlay, a `cancelled` sibling counts as satisfied for ω's edge; if the scheduler treats such an edge as unmet, the decompose steward removes it by hand and applies the stamp in a docs-only commit rather than leaving ω permanently blocked | **PASS** |

---

## Out-of-batch companion (not a PRD leaf, no manifest label)

The **type-param-defaults follow-up** (§9) is filed at decompose as a separate
low-priority task, gated on α. Its premise was verified 2026-08-20:
`convert_type_params` (`type_resolution.rs`) resolves a `Named` type-param default
as `resolve_type_name(name).unwrap_or_else(|| Type::StructureRef(name.clone()))` —
an unconditional `StructureRef` fallback that consults neither the shadow override
nor the enum fallback. The α gate is a **real semantic edge, not lock management**:
`convert_type_params` is called from eight sites, and after the hoist all of them
except the one inside `pre_pass` sit inside the shadow scope, whereas today the
aliases/functions/traits sites do not — so a thread-local-consulting fix authored
before α would behave inconsistently across call sites.

## Bindings that had to be resolved

None. Every binding above resolved to PASS on first evaluation; no signal was
re-homed, no bound relaxed, no prerequisite newly queued. The PRD was authored
the same day against empirically-probed substrate, and this manifest is a
confirmation pass rather than a repair pass.
