# PRD: Compiler module namespace — explicit imports, one resolution context, one per-sub record, one Type traversal

Status: **draft (pass 1, uncommitted)**. Authored 2026-10-12 by the S7 `/prd` seat of program
`review-all-reify-20261010` (program doc `plans/review-all-program-reify-20261012.md`, stream
**S7 Compiler module namespace**, wave 1, mode agent, size L, risk low). Shape: **B + H-lite**
(contracts §4, boundary-test sketch §5). Milestone: v0_6.

> **Code anchors** verified against main `8e6ffcac9d` (2026-10-12). The program's freshness
> anchor is `a030ee7528` (2026-10-10); `git diff a030ee7528..main` over
> `crates/reify-compiler/src`, `crates/reify-core/src`, `crates/reify-syntax/src`,
> `crates/reify-eval/src/persistent_cache.rs` and `crates/reify-constraints` is **empty**, so
> every measurement below holds at both commits. Cite-by-symbol is the default; the few
> line ranges quoted are dated by this header. Re-locate at implementation time.

---

## §0 — Provenance and rulings

Program ruling *S7 Compiler module namespace* (human, 2026-10-12; do not relitigate): replace
the crate-root `pub(crate) use X::*` globs and the `use super::*` imports with explicit
`use crate::module::{names}`; move the builtin tables out of `units.rs` into
`builtin_families.rs`; a `ResolutionCtx` argument replaces the three thread-local resolution
scopes; `CompilationScope`'s `sub_*` maps become one per-sub record behind query methods;
`Type::children()`/`any_descendant()` in reify-core replaces the three identical any-descendant
walkers. The stream row adds: delete the four re-export shims; perform the heuristic-14
partitions of `expr.rs` (with S8), `type_compat.rs` and `units.rs`; enforce with a ratchet on
glob imports and glob re-exports.

Rulings that bind this PRD, by name in the program doc §Resolved decisions: *S7*,
*Contradictions* (the four namespace findings f240/f196/f206/f277 are **medium**, sites of one
remedy — this overrides the `high` still recorded on fk-28fab6d836c8), *Invariant candidates*
(candidate 7, INV-ARCH-1, is co-owned S3 + S7; candidate 5, extract-with-ratchet, is
repo-wide), *Weakened findings*, *Filing stance*.

Finding keys owned (11): fk-28fab6d836c8, fk-78d6c776ba64, fk-6460b80f5442,
fk-120b6255ec1c, fk-a9fb39978c17, fk-eeae7ace09ad, fk-bde20c8c3ed4, fk-fc9a2fe8498b,
fk-39797230491b (filed #8552), fk-6432e37b9f24 (filed #8551), fk-6875d651562b (filed #8560).
Each key is carried by exactly one leaf or adopted task in §8.

Heuristics applied (dark-factory `docs/code-quality.md`): 13 *files make internal sense in
isolation* (the glob namespace, the shims), 9 *deep modules with appropriate nesting* (the
entity↔guards peer cycle), 7 *prefer stateless interactions* (the thread-local scopes), 5
*minimum data access scopes* and 10 *clear invariants, redundantly, uniformly enforced*
(`CompilationScope`), 11 *SPOT* (the Type walkers), 1 *informative names* and 6 *well-defined
purpose* (`units.rs`), 14 *no file too large* (the partitions), and the Comments stance (the
install-site lists and "why a thread-local" essays this PRD deletes).

## §1 — Goal and consumers (G1)

After this PRD lands, an agent reading any one reify-compiler source file can tell from its
`use` lines which module or crate every callee lives in; a new peer cycle or a new glob is red
at the gate; whether a type resolution sees enum names is a field of the argument it was
passed, not an accident of which caller installed a guard; a new per-sub fact is one field on
one record built in one constructor; a new `Type` variant is classified once.

Consumers (every mechanism below names one):

| Mechanism | Consumer |
|---|---|
| Explicit imports + glob deletion (β–κ) | **S9** (compiler declaration passes; lands after S7 per the program's compiler order S7 → S9 → S8 → S12) edits `entity.rs`, `traits.rs`, `forall_elaborate.rs`, `guards.rs`; **S8** edits `expr.rs`'s FunctionCall ladder and the builtin tables; every future compiler task (the next-change scenarios of all four namespace findings). |
| `builtin_families.rs` (ε) | **S8** — its upstream seam in the program table is literally "S7 (explicit imports; builtin tables out of units.rs)"; S8 then migrates the families into reify-builtins rows. |
| Namespace ratchet member (α) | The gate (`tests/infra/run_all.sh`), read by every compiler change. |
| INV-ARCH-1 reify-compiler rows (ξ) | The gate; S3 (co-owner) reads the same tool for reify-eval/reify-ir. |
| `ResolutionCtx` (μ) | Every type-resolution call site (29 production callers outside `type_resolution.rs`); #6893 (lambda-param annotations resolve with an explicitly empty context today) and #6399 (type-param defaults bypass the shadow override) get a context value to pass instead of a guard to install. |
| `SubFacts` record (ν) | `expr.rs`, `connect.rs`, `member_path.rs` readers; the next connect check (fk-a9fb39978c17's next-change scenario). |
| `Type::children` / `any_descendant` (λ) | `reify_core::overload::{type_carries_type_param, type_carries_dim_param}` and `type_mentions_conflicted_param`; the next `Type` variant. |
| `satisfies_trait_bound` root path (C3) | **S12**'s deletion of `reify_eval::tolerance_combine::conforms_to_trait`. |

No language surface changes: no grammar, builtin, diagnostic, CLI or GUI behaviour is added or
altered, so the overlay's docs-truth gate does not fire (no chunk, corpus or cheatsheet leaf).

## §2 — Current state at main (measured 2026-10-12)

- `crates/reify-compiler/src/lib.rs` carries **28** `pub(crate) use <mod>::*;` lines, 5 under
  `#[allow(unused_imports)]`, preceded by the comment "Re-export submodule items for internal
  cross-module access via `use super::*;`". It also carries `pub use types::*;` (the crate's
  public type surface, kept — see D9).
- **25** of 56 top-level `src/*.rs` files open with column-0 `use super::*;` (annotations,
  coerce, connect, datum_projection, diagnostics, entity, expr, forall_elaborate, functions,
  geometry, geometry_boolean, geometry_curve, geometry_list, geometry_modify,
  geometry_transform, guards, ice, member_path, scope, termination, trait_requirements,
  traits, type_compat, type_resolution, units). Three more files reach the root globs through
  `conformance/`: `conformance/mod.rs` (`use super::*;`, plus a sibling `use checker::*;`),
  `conformance/sub_component_validation.rs` (`use super::*;`, transitively the root) and
  `conformance/checker.rs` (`use crate::*;`). Total column-0 non-test glob imports: **29**.
- **Peer cycle**: `entity.rs` calls `guards.rs` 6 times (`compile_block_guard`,
  `register_guarded_names` ×2, `compile_per_decl_guard` ×3); `guards.rs` calls back into five
  `entity.rs` helpers — `build_param_value_cell_decl`, `build_auto_let_value_cell_decl`,
  `resolve_auto_let_cell_type`, `fixup_option_none_for_let`, `fixup_option_none_for_param`.
  Those helpers depend only on `annotations.rs` (`validate_annotations`, `lower_annotations`,
  `extract_solver_hints`, `validate_solver_hint_collections`), `annotations/display.rs`, the
  resolver, and one further `entity.rs` fn, `reject_keyed_value_position`.
- `type_compat.rs` (5,299 lines; production 1–1912) keeps a facade
  `pub(crate) use reify_core::overload::{type_carries_trait_object, type_carries_type_param};`
  whose own comment says it survives only for `expr.rs`,
  `compile_builder/entities_phase.rs` and `variant_construct.rs` — tracked as **#7338**.
- `units.rs` (4,585 lines): lines 1–1,695 are the builtin catalogue (16 name tables, the
  `is_*` predicates and `*_result_type` fns, selector composition); 1,696–1,910 are units
  (`unit_to_scalar`, `UnitEntry`, `UnitRegistry`, `UnitResolveError`,
  `unit_resolve_error_to_diagnostic`, `resolve_unit_expr`); tests from 1,911. `lib.rs`
  re-exports 12 catalogue items (10 name tables, 2 result-type fns) publicly for reify-eval's
  `registry_drift_tests`.
- `type_resolution.rs` holds three `thread_local!` sets — `RESOLUTION_ENUM_NAMES`,
  `RESOLUTION_SHADOWING_ENUM_NAMES`, `ALIAS_DEFER_IN_PROGRESS` — with RAII guards
  `EnumNameScope`, `LocalEnumShadowScope`, `AliasDeferScope`. Production installs:
  `EnumNameScope` ×5 (`defs_phase::phase_constraint_defs`, `entity.rs` struct-param
  resolution, `functions.rs` ×2, `validate_pub_parametric_alias_def_site`) against a doc that
  says "exactly four"; `LocalEnumShadowScope` ×1 (`compile_with_prelude_context_checked_with_config`);
  `AliasDeferScope` ×1. All three are read in `resolve_entity_name_with_shadowing` /
  the deferred-alias arm. Callers outside the file: `resolve_type_expr_with_aliases` 23,
  `resolve_type_expr_with_aliases_kinded` 6.
- `CompilationScope` (`scope.rs`, 850 lines) has 30 `pub(crate)` fields, 8 of them `sub_*`
  maps (`sub_component_types`, `sub_structure_traits`, `sub_assoc_fn_keys`,
  `sub_member_types`, `sub_realization_names`, `sub_port_directions`,
  `sub_declared_member_names`, `sub_match_arm_groups`), populated at two Sub pre-pass sites in
  `entity.rs` (the plain Sub pre-pass and the MatchArmDeclGroup pre-pass, the latter folding
  `sub_port_directions`/`sub_declared_member_names` over arms) and read from `expr.rs`,
  `connect.rs`, `member_path.rs`. Two keyings coexist: `sub_structure_traits` and
  `sub_assoc_fn_keys` are keyed by **structure** name, the rest by **sub** name. Tracked in
  part by **#8006** (one resolved-child record) and **#7986** (move the projection helpers).
- reify-core `Type` has no shared child traversal. `type_carries_type_param` and
  `type_carries_dim_param` (`reify_core::overload`) and `type_mentions_conflicted_param`
  (`type_compat.rs`) are byte-for-byte the same exhaustive constructor classification with
  different leaf predicates; `type_carries_trait_object` is deliberately narrower and pinned by
  `type_carries_trait_object_covers_its_narrower_walk`. The any-descendant arm shape
  `Type::Projection { base, .. } => <fn>(base` occurs **4** times in production.
- Shims: `reify-syntax` `pub use reify_ast::*;` (0 code consumers; #8552);
  `reify_eval::persistent_cache` six `pub use` shims (~45 old-path uses; #8551);
  `reify-constraints` loop-closure re-exports, the sole reason for its `reify-stdlib` Cargo
  edge (5 test consumers; #8560); the `type_compat` overload facade (3 modules; #7338).

## §3 — Resolved design decisions

**D1 — Migration order: consumers first, globs last.** Explicit imports coexist with the root
globs (an explicit name shadows a glob), so each leaf converts a disjoint set of consumer
files while the 28 globs stay; the globs are deleted in one leaf (κ) once no column-0 glob
import remains. Every per-file leaf lowers the ratchet baseline in the same diff. This keeps
every intermediate main green and keeps file locks narrow.

**D2 — The entity↔guards cycle is broken by a lower module, not by inlining.** The five
cell-builder helpers guards.rs takes from entity.rs (plus `reject_keyed_value_position`, which
they call) move verbatim to a new `member_cell_decls.rs` that imports only `annotations`,
`annotations::display`, the resolver and `types`; both `entity.rs` and `guards.rs` import it.
Afterwards `guards.rs` names no `crate::entity` item.

**D3 — New modules are siblings importing downward, never children reaching back.** Every
module this PRD creates (`member_cell_decls`, `builtin_families`, `type_unification`,
`overload_resolution`, `operator_typing`, `poison`, the `expr.rs` partitions) is a top-level
`mod` in `lib.rs` whose imports point at lower modules only. A file that would need to import
from the file that imports it is not split (heuristic 14's no-cheating clause). The one
exception is the conventional `#[cfg(test)] mod tests;` child file, whose `use super::*` is the
Tests-stance test seam and is excluded from the ratchet (C7).

**D4 — `expr.rs` partition split with S8.** The heuristic-14 measurement names six partitions.
This PRD performs five: poison helpers, reserved member/combinator names, empty-collection
typing, member-access helpers, and the tests. The member-access helpers move because none of
them recurses into `compile_expr_guarded` (measured: 0 calls in the helper bodies); the
MemberAccess **arm body**, which compiles the receiver and the index (its 2 recursive calls),
stays in the `expr.rs` dispatcher and calls the helpers downward. The **FunctionCall arm**
partition (the builtin-typing ladder) is S8's: S8's registry migration collapses it, and
splitting it now would be a recursive-descent reach-back that heuristic 14 forbids. `expr.rs`
will remain above the 2,000-line alarm until S8 lands; this PRD records that, it does not hide
it.

**D5 — `units.rs` keeps only units; the catalogue moves verbatim.** `builtin_families.rs`
receives lines 1–1,695's symbols unchanged (names, bodies, doc text). `lib.rs`'s
`pub use units::{…}` is retargeted so every `reify_compiler::<NAME>` public path is
unchanged. Behaviour changes inside the catalogue (fk-eeebffcc4a83 `is_surface_producing_arg`,
the family/registry migration) are S8's, not this PRD's.

**D6 — `type_compat.rs` splits four ways.** `type_compat.rs` keeps the implicit-conversion and
compatibility predicates (its name fits them); `type_unification.rs` takes `TypeArgConflict`,
`unify`, `type_mentions_conflicted_param`; `overload_resolution.rs` takes
`OverloadResolution`, `resolve_function_overload`, `try_default_padding`,
`format_fn_signature`; `operator_typing.rs` takes the operator group (`resolve_binop` …
`format_dimension_mismatch_diagnostic`). Each takes its share of the 3,386-line test module as
a child test file. The facade re-export is deleted and its three consumers import
`reify_core::overload` directly. `pub use type_compat::{implicitly_converts_to,
type_compatible}` stays.

**D7 — `ResolutionCtx` is an immutable value; the alias stack is a persistent list.** One
struct carries everything a resolution reads besides its `TypeExpr` and its diagnostics sink:
the alias registry, the structure and trait name sets, the type-param and dim-param sets, the
enum-fallback set (replacing `RESOLUTION_ENUM_NAMES`; absent = today's "not installed"), the
local-enum shadow set (replacing `RESOLUTION_SHADOWING_ENUM_NAMES`), and the in-progress alias
stack (replacing `ALIAS_DEFER_IN_PROGRESS`) as a borrowed linked list, so entering an alias
returns a child context and a cycle is "name already on the stack" — still cycle-exact.
Contexts derive by value (`with_type_params`, `with_enum_fallback`, `entering_alias`), never by
mutation (heuristics 7, 8). The kinded/non-kinded entry pair collapses into one entry because
`dim_params` is a context field defaulting empty. The behaviour of every existing install site
is preserved exactly: the five `EnumNameScope` sites construct their context with the enum
fallback, every other site without it — this PRD does **not** decide whether enum fallback
should become uniform (that would be a behaviour change).

**D8 — `SubFacts` encodes the absence contracts as types.** One map keyed by sub name holds a
record per sub: the structure name plus `Option<ResolvedSub>` — `None` meaning
"forward-declared, child template not compiled yet", which today is inferred as
`sub_component_types.contains_key && !sub_member_types.contains_key`. A second map keyed by
structure name holds the per-structure facts (`trait_bounds`, `assoc_fn_keys`). Both are
private fields of `CompilationScope`, filled by two methods — one for a plain sub (one
constructor from the resolved child template) and one for a match-arm cluster (the per-arm
fold rules for port directions and declared member names live in that one method) — and read
through query methods. Every reader in `expr.rs`, `connect.rs`, `member_path.rs` and the
scope.rs tests moves to the queries. The geometry-list pair and the match-arm key parity named
by the same finding are **not** in the ruled remedy (lead ruling 3: recorded as a follow-up
candidate in §11).

**D9 — `pub use types::*` stays.** It is the crate's public type surface for other crates, not
the internal namespace mesh. Internal modules import from `crate::types` explicitly like any
other module. The ratchet allowlists exactly this one line.

**D10 — Enforcement: the ruled ratchet plus a construction-time lint at the end.** A Python
infra member counts the two glob forms against a committed exact baseline (red on growth, red
on an un-lowered baseline), from the first leaf. When the counts reach zero (κ), reify-compiler
also gains `#![deny(clippy::wildcard_imports)]` (the gate runs `cargo clippy --all-targets -- -D
warnings`), which reports a new glob at its line with a fix, at construction — heuristic 10's
"redundantly". The ratchet stays as the gate-side check.

**D11 — Type traversal: one exhaustive classification, predicates over it.** `Type::children`
is the single exhaustive match naming which variants carry inner types and in what order
(the exact set the three walkers share today); `any_descendant(pred)` is `pred(self) ||
children().any(…)`. The three predicates become one-liners. `type_carries_trait_object` keeps
its explicit narrower walk. The transforming walkers (`unify`, `substitute_type_params`,
`normalize_type_guarded`, `walk_type_for_applied`, Display) are out of scope (D-OOS-2).

## §4 — Contracts (H)

**C1 — Public surface of reify-compiler is unchanged.** The set of `reify_compiler::<path>`
items reachable from other crates is identical before and after every leaf: the existing
`pub mod`s, `pub use containment_graph::sub_component_forward_adjacency`,
`pub use annotations::materialize::{…}`, `pub use compile_builder::pre_pass::check_module_path_decl`,
`pub use entity::satisfies_trait_bound`, `pub use prelude_context::PreludeContext`,
`pub use type_compat::{implicitly_converts_to, type_compatible}`, `pub use types::*`, the
12-item catalogue `pub use` (re-sourced from `builtin_families` in ε, `UnitEntry`,
`UnitRegistry`, `UnitResolveError`, `resolve_unit_expr` still from `units`), and
`pub use unresolved_function::is_known_builtin`. The workspace build (reify-eval,
reify-lsp, reify-cli, gui) is the boundary test.

**C2 — `builtin_families.rs` for S8.** Holds exactly the symbols `units.rs` lines 1–1,695
defined at main `8e6ffcac9d`, same names and signatures; imports only `reify_core`,
`reify_ir` and named lower modules (no glob). S8 owns every later edit to it.

**C3 — `reify_compiler::satisfies_trait_bound(&[String], &str, &HashMap<String,
&CompiledTrait>) -> bool`** keeps this root path and signature through every S7 leaf. S7 does
not move its definition (`entity.rs`). S12 may depend on it at any point.

**C4 — `ResolutionCtx`** (crate-private, `type_resolution.rs` or a lower module it owns):
- constructed from module-wide names once per compile phase and refined per declaration;
- carries: alias registry, structure names, trait names, type params, dim params,
  `enum_fallback: Option<set>`, local-enum shadow set, alias stack;
- every resolver that today takes any of these sets (or reads a thread-local) takes one
  `&ResolutionCtx` in their place, plus its subject (`&TypeExpr` or name) and the
  `&mut Vec<Diagnostic>` sink;
- no `thread_local!`, `RefCell` or `Drop` guard remains in type resolution;
- an explicitly empty context has a named constructor; every site that passes empty name
  sets today calls it, so a later fix (#6893 lambda-param annotations, #6399 type-param
  defaults) swaps in a real context instead of installing a guard;
- equivalence: for every call site, the context it builds yields the same `Type` and the same
  diagnostics, in the same order, as the thread-local state at that site did.

**C5 — `SubFacts`**: as D8. Queries cover every read the 8 maps serve today (structure name of
a sub, forward-declared?, member type, realization-name membership, port direction, declared
member names, match-arm clusters, structure trait bounds, structure assoc-fn keys). No
`pub(crate) sub_*` field remains.

**C6 — `Type::children(&self)`** yields the direct inner types in the walkers' order (Function:
params then return; Map: key then value; Field: domain then codomain); `Type::any_descendant`
as D11. Exhaustive (no `_`). A completeness test over `TypeDiscriminants` (strum `EnumIter`,
already derived) fails when a variant is added without classification.

**C7 — Ratchet members** (baselines `tests/infra/compiler-namespace-ratchet-baseline.txt` and `tests/infra/type-walker-ratchet-baseline.txt`; Python stdlib `unittest`, each with a thin `test_<name>.sh` wrapper
and a `run-all-classification.manifest` row `pool`, per reify `CLAUDE.md` §Infra tests):
- `tests/infra/test_compiler_namespace_ratchet.{py,sh}` — counts over non-test
  `crates/reify-compiler/src` code: (a) visibility-qualified glob re-exports
  (`pub use …::*;` / `pub(crate) use …::*;`), allowlisting exactly `lib.rs`'s
  `pub use types::*;` (D9), baseline 28; (b) unqualified column-0 glob imports
  (`use …::*;`), baseline 29. Test code is excluded by construction: indented `use` lines in
  inline `mod tests` are not column-0, and files that are `#[cfg(test)]` children are skipped
  (the member checks each skipped file really is declared under `#[cfg(test)]`). The baseline is a committed file; the member is red
  when a count exceeds it (naming each offending `file:line`) and red when a count is below it
  (naming the stale row), so every leaf lowers it in the same diff.
- `tests/infra/test_type_walker_adoption_ratchet.{py,sh}` (INV candidate 5) — counts the
  any-descendant arm shape `Type::Projection \{ base, \.\. \} => [a-z_]+\(base` over production
  code in `crates/*/src`, baseline 4 → 1 after λ (the deliberate `type_carries_trait_object`).
- Shared counting code lives in one helper module both import (heuristic 11). Hermetic
  fixtures prove each member flags and clears (scratch tree: add one glob → red; remove it →
  red until the baseline is lowered; lower it → green) — a construct lint, not a prose pin
  (`tests/infra/README.md` → "Assert behaviour, not source prose"). No wall-clock bound.

**C8 — INV-ARCH-1 reify-compiler rows** (ξ): once κ has landed, the module-SCC ratchet
(candidate 7's checkable form: production-text `crate::<module>` edges, comments stripped, cut
at the first top-level `#[cfg(test)]`, Tarjan SCC, per-crate baseline of SCC sizes, red on
growth) gains reify-compiler with its measured residual SCCs as the baseline; `entity` and
`guards` are not in one SCC. Until then the tool must exclude reify-compiler (or model a root
glob import as edges to every glob-re-exported module): today's globs hide every edge, so
measuring the crate before κ would read near-empty and then "grow" as each leaf makes edges
visible.

## §5 — Boundary-test sketch

| # | Scenario | Pre | Post (asserted) |
|---|---|---|---|
| 1 | Behaviour floor, every leaf | main green | full reify-compiler suite, `examples_smoke`, the prd-gate corpus and the workspace build green with **no** edit to any expected diagnostic, snapshot or test expectation; a leaf that needs one is out of scope and stops |
| 2 | Glob growth | ratchet at N | adding one `use super::*;` in a non-test compiler file → member red naming the file:line |
| 3 | Un-lowered baseline | a leaf removes k globs | member red until the baseline file drops by k |
| 4 | Cycle broken (β) | entity↔guards mutual calls | `guards.rs` has no glob and imports no `crate::entity` item; after ξ, the SCC listing shows `entity` and `guards` apart |
| 5 | Public surface (C1) | reify-eval `registry_drift_tests` reads `reify_compiler::GEOMETRY_FUNCTION_NAMES` etc. | compiles unchanged after ε |
| 6 | Alias cycle (μ) | `tests/prd-gate/fixtures/s7_resolution_alias_cycle.ri`: `reify check` → rc 1, `error: circular type alias 'C1'`, `error: unresolved type: C1` | identical output after μ |
| 7 | Enum inside a parameterized builtin (μ) | `tests/prd-gate/fixtures/s7_resolution_enum_in_option.ri` (`param m : Option<Mode>`): rc 0, `All constraints satisfied.` | identical after μ |
| 8 | Local enum shadows a prelude structure (μ) | `docs/prds/v0_6/fixtures/member_enum_ctor.ri` rc 0 `OK Test#constraint[0]`; `shadow_payload_binder.ri` rc 0; `harness_langcore/enum_ctor_param_binding_tests.rs` green | identical after μ, with the tests rewritten only where they installed a scope guard directly |
| 9 | Forward-declared sub (ν) | `sub_member_is_cross_sub_geometry_or_forward_declared` tests | same truth table through the `SubFacts` query |
| 10 | New Type variant (λ) | — | the `TypeDiscriminants` completeness test fails until `children` classifies it; the three predicates agree with their pre-λ results on a corpus of nested types built in the test |

## §6 — Cross-PRD relationship (G4)

| Other stream / PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| S7 (this) | produces | `crates/reify-compiler/src/lib.rs` re-export block and migration order | **S7** (program G4 table) | this PRD |
| S12 check surface | consumes | `reify_compiler::satisfies_trait_bound` root path + signature (C3) | **S7** | stable from now; no S7 leaf blocks S12 |
| S8 builtin registry (wave 2) | consumes | `builtin_families.rs` (C2), explicit imports in `expr.rs`, `operator_typing.rs`; performs the FunctionCall-arm partition of `expr.rs` (D4) | S7 produces; S8 owns later edits | S8 leaves touching these files depend on ε / θ / δ |
| S9 declaration passes (wave 1, after S7) | consumes | explicit imports in `entity.rs`, `traits.rs`, `forall_elaborate.rs`, `guards.rs`; `member_cell_decls.rs` | S7 | S9 leaves on those files depend on β / θ |
| S3 dependency stratum (wave 1) | co-owns | INV-ARCH-1 module-SCC ratchet (candidate 7) | **S3 α** builds the tool and promotes INV-ARCH-1 (lead ruling 1); S7 ξ adds the reify-compiler rows | ξ held on S3 α |
| S27 tooling front end | adjacent | μ edits the body of `compile_with_prelude_context_checked_with_config` (the shadow-scope install becomes a context construction); no signature change | S27 owns the compile entry signatures | coordinate by dependency if both are in flight |
| S6 kinematics | adjacent | #8560 repoints 5 tests from `reify_constraints::{JointValue, …}` to `reify_stdlib::loop_closure_value` / `loop_closure_solver` | S6 owns the JointKind/JointValue home | #8560 lands first; S6 re-touches those imports if it moves JointValue |
| S5 analysis stratum | adjacent | #8551 repoints to `reify_compute_contract::{ElasticResult, …}` | S5 owns reify-compute-contract | #8551 lands first |
| S23 prose rehoming (wave 4) | adjacent | μ/ν delete the install-site lists and "why a thread-local" essays outright | S7 for prose it deletes | — |
| `docs/prds/v0_6/enum-shadow-coherence.md` (#6955, pending) | upstream of μ | #6955 M3(a) adds a reader of `RESOLUTION_SHADOWING_ENUM_NAMES` in `conformance/checker.rs` | that PRD | **μ depends on #6955** so μ converts the new reader too instead of invalidating #6955's premise |
| `docs/prds/v0_6/stdlib-namespace.md` (#5493 α, pending; #5494 γ, pending) | upstream of μ; file contention | #5493 introduces `name_env.rs` and owns the shadow-install fold comment in `lib.rs`; #5494 rewrites phase order in `lib.rs`/`entity.rs`/`functions.rs`/`type_resolution.rs` | that PRD | **μ depends on #5493**; #5494 is file contention only (orchestrator locks serialize) |

## §7 — Pre-conditions

None for α–λ: no novel substrate (no grammar, no new crate dependency — strum is already a
reify-core and reify-compiler dependency; clippy runs in the gate). μ waits on #6955 and
#5493 (§6). ξ waits on S3 α (INV-ARCH-1 tool and baseline; held leaf).

## §8 — Decomposition plan

Every leaf is behaviour-preserving (§5 row 1 is part of its done-criteria) and lowers the C7
baseline by the exact number of globs it removes. Signals name what the gate and an operator
see; "ratchet line" means the C7 member's printed counts in `run_all.sh` output.

- **α — Namespace ratchet member (C7 first member + shared helper).** Modules: `tests/infra`
  (new `.py` + `.sh` + baseline file + manifest row). Signal: `run_all.sh` lists
  `test_compiler_namespace_ratchet` green, printing `glob_reexports 28` and
  `glob_imports 29`; its hermetic fixtures go red on an added glob and on an un-lowered
  baseline. Consumer: β–κ, λ (shared helper). Prereqs: none. Keys: none (the ruled enforcement).
- **β — Explicit imports for guards/traits/forall_elaborate/geometry_list; break the
  entity↔guards cycle (D2).** Modules: reify-compiler (`guards.rs`, `traits.rs`,
  `forall_elaborate.rs`, `geometry_list.rs`, `entity.rs` helper removal, new
  `member_cell_decls.rs`, `lib.rs` `mod` line). Signal: ratchet line `glob_imports 25`;
  `guards.rs` imports `crate::member_cell_decls` and no `crate::entity` item; §5 row 1.
  Consumer: κ, θ, ξ; S9. Prereqs: α. Keys: contributes to fk-28fab6d836c8 (carried by κ).
- **γ — Explicit imports for the eight-file slice; privatise termination's dead helpers.**
  Modules: `coerce.rs`, `datum_projection.rs`, `diagnostics.rs`, `geometry_boolean.rs`,
  `geometry_curve.rs`, `geometry_transform.rs`, `ice.rs`, `termination.rs`
  (`termination_args_contain_undef`, `termination_collect_refs`, `termination_is_modifying`
  have no caller outside the file → private). Signal: ratchet line drops by 8;
  `termination.rs` reads with every name's origin on its `use` lines; §5 row 1. Consumer: κ.
  Prereqs: α. **Keys: fk-120b6255ec1c.**
- **δ — `type_compat.rs`: explicit imports, delete the overload facade, four-way split (D6).**
  Modules: `type_compat.rs`, new `type_unification.rs`, `overload_resolution.rs`,
  `operator_typing.rs` + child test files, `expr.rs` / `compile_builder/entities_phase.rs` /
  `variant_construct.rs` import lines, `lib.rs` `mod` lines. Signal: ratchet line drops by 1;
  `git grep 'type_compat::type_carries'` empty; each new file states its purpose in one
  sentence without "and"; §5 row 1. Consumer: θ, λ; S8 (operator group). Prereqs: α.
  **Keys: fk-78d6c776ba64. Subsumes task #7338** (decomposition either cancels #7338 as
  superseded by δ, naming δ's id, or folds it in as δ's own dependency).
- **ε — `builtin_families.rs`; `units.rs` keeps units (D5, C2).** Modules: `units.rs`, new
  `builtin_families.rs` + test split, `lib.rs` (`mod` line, `pub use` retarget). Signal:
  ratchet line drops by 1; `units.rs` defines no `*_NAMES` table; reify-eval
  `registry_drift_tests` compiles unchanged (§5 row 5); §5 row 1. Consumer: θ; **S8**.
  Prereqs: α. **Keys: fk-bde20c8c3ed4.**
- **ζ — Explicit imports for the remaining shared modules and `conformance/`.** Modules:
  `annotations.rs`, `connect.rs`, `member_path.rs`, `trait_requirements.rs`, `functions.rs`,
  `scope.rs`, `geometry_modify.rs`, `geometry.rs`, `conformance/{mod,sub_component_validation,
  checker}.rs` (including the sibling `use checker::*;`). Signal: ratchet line drops by 12
  (8 top-level + 4 in `conformance/`); §5 row 1. Consumer: κ. Prereqs: α. Keys: contributes to fk-28fab6d836c8.
- **η — Explicit imports for `type_resolution.rs`.** Modules: `type_resolution.rs`. Signal:
  ratchet line drops by 1; §5 row 1. Consumer: κ, μ. Prereqs: α. Keys: contributes to
  fk-28fab6d836c8.
- **θ — Explicit imports for `expr.rs` and `entity.rs`.** Modules: `expr.rs`, `entity.rs`.
  Signal: ratchet line drops by 2 (to 0 with β γ δ ε ζ η); the builtin-family predicates are
  imported from `crate::builtin_families` by name, so a reader of `expr.rs` is sent to the
  right file; §5 row 1. Consumer: ι, κ; S8, S9. Prereqs: β, δ, ε. **Keys: fk-6460b80f5442.**
- **ι — `expr.rs` heuristic-14 partition, five of six (D4).** Modules: `expr.rs`, new
  `poison.rs`, reserved-names module, empty-collection-typing module, `member_access.rs`,
  `expr/tests.rs` (tests + `member_access_completeness_canary`), `lib.rs` `mod` lines. Signal:
  each new file passes heuristic 13 (imports point downward; none imports `crate::expr`); the
  commit message records the six-partition measurement and that the FunctionCall arm stays for
  S8; §5 row 1. Consumer: ν; S8. Prereqs: θ. Keys: none (ruled h14 partition). **Subsumes task
  #8321** (move `expr.rs` tests to a child file).
- **κ — Delete the 28 root globs; deny `clippy::wildcard_imports` (D10).** Modules: `lib.rs`
  (the 28 lines, their comment, the 5 `#[allow(unused_imports)]`), `annotations/materialize.rs`
  and any other path that resolved through a root glob, crate attribute. Signal: ratchet line
  `glob_reexports 0` / `glob_imports 0`; the gate's clippy pass reds a newly added
  glob in reify-compiler at its line; §5 rows 1–3. Consumer: μ, ν, ξ, every later compiler
  change. Prereqs: β, γ, δ, ε, ζ, η, θ. **Keys: fk-28fab6d836c8** (medium per the
  *Contradictions* ruling).
- **λ — `Type::children` / `any_descendant` in reify-core (D11, C6); adoption ratchet.**
  Modules: `crates/reify-core/src/ty.rs`, `crates/reify-core/src/overload.rs`,
  `type_unification.rs`, `tests/infra` (second C7 member + manifest row). Signal:
  `test_type_walker_adoption_ratchet` green printing `any_descendant_walkers 1`; §5 row 10;
  §5 row 1. Consumer: the next `Type` variant; the three predicates. Prereqs: α (helper), δ.
  **Keys: fk-fc9a2fe8498b. INV: candidate 5 (extract-with-ratchet) — ratchet member
  `tests/infra/test_type_walker_adoption_ratchet.sh`.**
- **μ — `ResolutionCtx` replaces the three thread-local scopes (D7, C4).** Modules:
  `type_resolution.rs`, `compile_builder/{defs_phase,entities_phase,enums_phase}.rs`,
  `entity.rs` (11 call sites), `functions.rs`, `guards.rs`, `traits.rs`,
  `conformance/checker.rs` (6 call sites, plus #6955's shadow-set reader), `lib.rs` (install
  site body only). Signal: `reify check` on §5 rows 6–8
  fixtures produces byte-identical output; no `thread_local!` in `type_resolution.rs`;
  the "exactly four sites" list and the three "why a thread-local" essays are gone; §5 row 1.
  Consumer: every resolution call site; #6893, #6399. Prereqs: κ, η; **#6955, #5493**.
  **Keys: fk-eeae7ace09ad.**
- **ν — `SubFacts` per-sub record behind query methods (D8, C5).** Modules: `scope.rs`,
  `entity.rs` (both Sub pre-pass sites), `expr.rs`, `member_access.rs`, `connect.rs`,
  `member_path.rs`, and the projection helpers' new home. Signal: no `pub(crate) sub_*` field
  in `scope.rs`; one constructor call per pre-pass site; §5 row 9; §5 row 1. Consumer: the next
  per-sub fact. Prereqs: κ, ι. **Keys: fk-a9fb39978c17. Subsumes tasks #8006 and #7986.**
- **ξ — reify-compiler rows on the INV-ARCH-1 module-SCC ratchet (C8).** Modules: the SCC
  tool's baseline (built by S3 α; S7 adds only the reify-compiler rows and the one-time
  exception note in its commit). Signal: the gate's SCC member lists reify-compiler
  with its residual SCCs; `entity` and `guards` are in different SCCs; adding a
  `crate::entity` import to `guards.rs` reds it. Consumer: every later compiler change; S3.
  Prereqs: κ, β, and S3 α (held until S3 α is filed and released by the lead). **INV: INV-ARCH-1
  (candidate 7).**
- **π — derive `strum::EnumCount` on the nine geometry-kind
  enums.** Modules: `crates/reify-compiler/src/types.rs`. Signal: no `const ALL: [Self;` and
  no "Maintenance contract" paragraph in `types.rs`; `VARIANT_COUNT` reads `COUNT`; the
  `geometry_modify` CASES assert and reify-eval `compile_geometry_op_registry_completeness`
  still compile; §5 row 1. Prereqs: none. **Keys: fk-bb1ba38a6a46** (lead ruling 2).
- **ω — PRD close.** Stamp this PRD and its manifest `SHIPPED` with the landed leaf ids, the
  AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED map (overlay §PRD terminal status).
  Prereqs: every leaf above and the adopted tasks below. Signal: the committed header.

**Adopted existing tasks (filed by the review-all mechanical filing; not re-filed):**

| Task | Key | Disposition at decompose |
|---|---|---|
| **#8552** delete reify-syntax `pub use reify_ast::*` | fk-39797230491b | keep as filed; ω depends on it |
| **#8551** repoint persistent_cache shim imports, delete the six shims | fk-6432e37b9f24 | keep as filed; ω depends on it |
| **#8560** drop the reify-constraints loop-closure shim and its reify-stdlib Cargo edge (its own test keeps `reify-stdlib` as a dev-dependency) | fk-6875d651562b | keep as filed; ω depends on it |

**DAG**: α → {β, γ, δ, ε, ζ, η}; {β, δ, ε} → θ → ι; {β, γ, δ, ε, ζ, η, θ} → κ; {α, δ} → λ;
{κ, η, #6955, #5493} → μ; {κ, ι} → ν; {κ, β, SCC tool} → ξ; π independent; everything → ω.

**Filed task ids (pass 2, 2026-10-12):** α #8613 · β #8615 · γ #8617 · δ #8620 · ε #8622 ·
ζ #8623 · η #8625 · θ #8627 · ι #8631 · κ #8636 · λ #8638 · μ #8643 · ν #8650 ·
ξ #8657 (held: `x_seam_wait` S3 α = #8673; edge wired) · π #8661 · ω #8665. Adopted: #8552,
#8551, #8560. Superseded and cancelled: #7338 (by δ), #8321 (by ι), #8006 and #7986 (by ν).
Upstream edges: #6955, #5493 → μ.

## §9 — Out of scope

- D-OOS-1: behaviour changes anywhere — enum-fallback uniformity, the convert_type_params
  fallback (#6399), lambda-param real contexts (#6893), `is_surface_producing_arg`
  (fk-eeebffcc4a83, S8), operator parsing at the syntax boundary (fk-e187a67f4b32, S8).
- D-OOS-2: the transforming Type walkers (`unify`, `substitute_type_params`,
  `normalize_type_guarded`, `walk_type_for_applied`, Display) and a `map_children`.
- D-OOS-3: the FunctionCall-arm partition of `expr.rs` (S8); partitions of `entity.rs`,
  `geometry.rs`, `type_resolution.rs` (no S7 ruling).
- D-OOS-4: `pub use types::*` and any change to reify-compiler's public API (D9, C1).
- D-OOS-5: moving `satisfies_trait_bound`'s definition.

## §10 — Open questions (tactical)

1. Final names for ι's reserved-names and empty-collection-typing modules. Decide in ι; each
   name states what the module holds (heuristic 1).
2. `Type::children`'s return type (boxed iterator, small array, or a dedicated iterator type).
   Decide in λ; not a contract.
3. Whether `ResolutionCtx` lives in `type_resolution.rs` or a small lower module both it and
   `compile_builder` import. Decide in μ by heuristic 13.
4. How the SubFacts constructor and the existing projection helpers (`member_type_map_from_template`
   and siblings, #7986) are homed together. Decide in ν; the module imports `types` downward.

## §11 — Gate walk (author mode)

- **G1** every mechanism has a consumer (§1 table).
- **G2** every leaf names a gate- or operator-visible signal; refactor leaves pair the ratchet
  line with the behaviour floor; κ and ν/μ carry the end-to-end pins (§5).
- **G3** substrate verified at main: strum in reify-core/reify-compiler `Cargo.toml`; clippy
  `-D warnings` in `scripts/verify.sh`'s lint plan; no existing `ResolutionCtx`, `SubFacts`,
  `builtin_families`, `member_cell_decls`, `Type::children`; `TypeDiscriminants` derives
  `EnumIter`/`EnumCount`; the two new fixtures parse (`tree-sitter parse --quiet`, 0 ERROR) and
  produce the §5 outputs with `target/debug/reify` built 2026-10-11, after the last
  reify-compiler source commit (`3bfae4d2e0`, 2026-10-10).
- **G4** §6; every seam has an owner (lead ruling 1).
- **G5** blast radius ≥ 3 crates → B + H-lite (§4, §5).
- **G6** no numeric or exactness claim beyond byte-identical diagnostics, which each fixture
  row records from a real run; no rejection is asserted that is not already observed (row 6).
- **G7** (reify INV-SF/INV-AD/INV-PD families, advisory in author mode): no new `Undef`, no
  new diagnostic, no placeholder; μ removes a silent-resolution-by-omission hazard (INV-SF-3
  shape) rather than adding one. No waiver.
- **META** yes (lead rulings 1–5 applied).

### Lead rulings on the pass-1 questions (2026-10-12; program doc §Resolved decisions — pass 1 open questions)

1. **INV-ARCH-1 tool owner → (a).** S3 α builds the only tool, baseline and INV-ARCH-1 row,
   with reify-compiler excluded until S7 κ; S7 ξ adds the reify-compiler rows (consistent with
   S3 OQ-3). ξ is filed as a held leaf waiting on S3 α.
2. **fk-bb1ba38a6a46 → (a).** Assigned to S7 as π (program doc stream table amended).
3. **fk-a9fb39978c17 reach → (a).** ν does the `sub_*` record only. Follow-up candidate (not
   filed): fold `geometry_list_elements` / `geometry_list_rejected` into one enum-valued map
   and the `match_arm_groups` / `match_arm_group_arm_member_types` pair into one record, so
   the disjointness and key-parity invariants live in the type.
4. κ files at **medium** (Contradictions ruling).
5. The glob-import baseline is **29** (25 top-level + 4 in `conformance/`).
