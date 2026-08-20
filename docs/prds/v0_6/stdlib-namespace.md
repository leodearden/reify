# PRD: Stdlib Namespace & Module Architecture — strict imports, one collision rule, stdlib as ordinary modules

Status: **active**. Authored 2026-07-25 in an interactive `/prd` session (Leo + investigation
session `investigate-reify-1281708`, spawned from the 2026-07-24 language review, brief
`2026-07-24-stdlib-module-arch.md`). Shape: **B+H** (contract §3 + boundary-test sketch §7).
Milestone: v0_6.

> Every claim marked *(probe)* was verified 2026-07-24 against the 2026-07-22 debug binary
> (probe files in the investigation session's scratchpad: `mode_probe.ri`, `enum_probe.ri`,
> `asym/`). Line numbers are against `main` at `366c63a679`. Implementers: rebuild before
> re-probing — `reify` embeds the stdlib via `include_str!`, so a stale binary carries a
> stale stdlib.

---

## §0 — Goal and user-observable surface

Reify's stdlib is compiled as a hand-ordered "growing sequential prelude" (~20 comment-only,
machine-unchecked ordering constraints in `stdlib_loader.rs`), and all 48 modules merge into
one flat ambient namespace whose collision behavior differs per name kind, per direction,
and between compile and eval. Both structures actively mis-serve users today *(probes below)*
and collapse under the standard-parts library program (#5391, hundreds of new names).

This PRD makes the stdlib **a set of ordinary modules** — declared imports, machine-checked
order, one collision rule shared by compile and eval — and makes user code **strict-imports
by default** with a one-line `import std.prelude` escape valve and real qualified references.

After this PRD lands, a user observes:

- Their own `enum SignalKind { Foo, Bar }` actually binds: `SignalKind.Foo` resolves.
  Today the stdlib's `SignalKind` **silently shadows the user's own declaration** —
  `SignalKind.Foo` errors, `SignalKind.Analog` resolves *(probe)*.
- `Mode` means the same thing at compile time and eval time. Today compile-side resolution
  is **last**-wins (modal `Mode`) while eval-side is **first**-wins (buckling `Mode`) —
  buckling's `Mode` fields are unreachable from user code (`m.eigenvalue` →
  `error: structure 'Mode' has no member 'eigenvalue'`) *(probe)*. Buckling's struct is
  renamed `BucklingMode`; a collision like it can never silently recur (stdlib build error).
- A file that uses `Length`/`10mm` without imports gets a fix-it diagnostic naming the
  providing module ("`Length` is defined in `std.units` — add `import std.units` or
  `import std.prelude`") instead of ambient magic; `import std.prelude` alone restores the
  scripting experience in one visible, versionable line.
- `import parts as pp` followed by `pp.Pulley` (expression and type position) resolves —
  today both are parse errors *(grammar-gate probe, fixtures
  `/tmp/prd-gate-fixtures/stdlib-namespace-{1,2}.ri`, both exit 1)*.
- Same-module stdlib authoring works without file-topology contortions: the four `*_fns`
  split files disappear; a struct default referencing a same-module fn stops silently
  evaluating a poison value (the task-3895 skeleton mirror-pass and its two documented
  silent-divergence hazards are deleted).

**Primary consumers:** the standard-parts & materials library program (**#5391** — its
gate (3) "working re-export/prelude story" second half, plus name-collision survivability
for hundreds of part/material names), and the **resolution-unification** program
(`docs/prds/v0_6/resolution-unification.md`) whose DefEnv needs compile-side semantics that
agree with it (its D-6/I6 normalize eval-side policy; this PRD owns the compile side and
the intra-stdlib policy).

---

## §1 — Background: the evidence

**Smell 1 — hand-ordered growing prelude.** `stdlib_sources()`
(`reify-compiler/src/stdlib_loader.rs:49-438`) hand-orders 48 modules; each compiles against
*everything* registered before it (`stdlib_topo.rs:163-196`). No stdlib module declares
`import`, so the topo-sort is the identity, pinned by
`signal_2_real_stdlib_compiles_clean_and_order_is_stable`. ~20 "MUST precede/follow"
constraints exist only as comments; adding any real `import` **reorders the sort** and may
silently violate the rest (the documented footgun, stdlib_loader.rs:37-48) — the machinery
punishes migration toward declared deps. `std.determinacy.purposes` MUST-be-last is
semantically load-bearing (`merge_prelude_purposes` runs during intra-stdlib compiles,
lib.rs:367-376): the growing-prelude model cannot express "prelude for users, not for
sibling stdlib modules".

**Smell 1b — phase order forced file topology.** Within one module compile,
`phase_functions` (lib.rs:579) runs before `phase_entities` (lib.rs:601), so fn bodies
compile before their own module's templates exist. Task 3895's
`build_structure_def_skeleton` (entity.rs:5951) is a **mirror pass** — a second, partial
template builder with two documented, accepted silent-divergence hazards: poison defaults
baked into ctor sites with diagnostics discarded (entity.rs:5935-5950) and
nested-ctor-in-`let` lowering to the wrong expression kind (entity.rs:5972-5984). Tasks
3895 + 4544 delivered same-module capability (`solver_elastic.ri:717`: "The same-module
ctor default `ElasticOptions()` now works" — struct + fns + defaults in ONE module), so
three of the four `*_fns` split files (`solver_buckling_fns`, `modal_analysis_fns`,
`trajectory_fns`) are vestigial; `modal_mechanism_fns` is a *cross*-module ordering
artifact (needs `std.modal.analysis` + `std.kinematic`) curable only by declared imports.

**Smell 2 — flat namespace, per-kind contradictory collision policies.** No name→module
binding exists anywhere: `Type::StructureRef(String)` carries a bare name (expr.rs:6532)
resolved via flat maps. Census (compile side):

| Kind | prelude-vs-prelude | local-vs-prelude | Diagnostic |
|---|---|---|---|
| structure templates | **last**-wins (`HashMap` collect: entities_phase.rs:120-125, functions_phase.rs:64-69) | local wins | silent |
| enums | source order | **prelude shadows local** (`prelude ++ local` + `.iter().find`: enums_phase.rs:286-287, type_resolution.rs:1199) | silent, **inverted** |
| units | **last**-wins | — | Warning |
| type aliases | first-wins | — | Warning |
| functions | first-wins (name+sig) | user first | silent |
| purposes | first-wins | local wins | silent |

Eval-side template lookup is **first**-wins (`find_template_with_prelude`,
reify-eval/src/engine_eval.rs:64-68) — compile and eval disagree about what `Mode` means
*(probe)*. The `modal_analysis.ri:142-145` comment claiming "first-wins … makes this safe"
is wrong on the compile side. resolution-unification's D-6 normalizes the **eval** side;
its θ/ι/κ tasks do not touch these compile-side sites (scope fence recorded in its §6).

**Smell 2b — the module system half-exists in the parser only.** `ImportKind` supports
selective (`import m.{A,B}`), aliased (`import m as x`), entity-aliased, and `pub import`
forms (reify-ast/src/decl.rs:704-733) with zero reify-compiler consumers (parser/LSP/tests
only). resolution-unification implements the exposure semantics (its P3); **qualified
references are this PRD's** (their §9 explicitly assigns them here). `pp.Pulley` fails the
grammar in both expression and type position *(grammar-gate probe)*.

**Smell 2c — two stdlib delivery regimes.** `std.*` resolves either from the embedded
compiled slice or per-module from a `stdlib_root` directory, policed by the all-or-nothing
`StdlibMode` machinery + three partial-overlay diagnostics (module_dag.rs:88-230). Fs mode
is broken for any std module with cross-std deps (modules compile against declared imports
only — there are none) and has no known real consumer beyond one leaf-override test.

---

## §2 — Sketch of approach

Five phases. N0 is independent bug-fix tier; N1 is compiler-internal; N2 delivers the
stdlib-as-ordinary-modules end-state; N3 flips user code to strict; N4 adds qualified
references. N3/N4 consume resolution-unification substrate (§5).

- **N0 — one collision rule, compile side.** Align every compile-side lookup to
  local-shadows-imported with deterministic first-wins among prelude peers (matching eval),
  fix the enum inversion, then make **intra-stdlib pub-name collision an Error at stdlib
  build** (Leo 2026-07-24) — which forces and includes the `Mode` → `BucklingMode` rename.
- **N1 — phase-order fix.** Build authoritative templates before fn-body compilation;
  delete the skeleton mirror; fold the three vestigial `*_fns` files.
- **N2 — stdlib as ordinary modules over an embedded mount.** Generate + commit real
  `import` headers for all stdlib modules (tooling reused for user migration); compile each
  stdlib module against **only** its declared imports (order derived, comments deleted,
  identity-order test replaced by a determinism test); resolve `std.*` through the same
  `ModuleDag` as user modules from an embedded read-only source root — `StdlibMode` and the
  overlay diagnostics become unrepresentable and are deleted. Fold `modal_mechanism_fns`.
- **N3 — strict-by-default + `import std.prelude`.** Ship the curated `std.prelude` facade
  (built on resolution-unification's `pub import` re-export, their μ); flip user code to
  strict visibility (builtins + declared imports only) with fix-it diagnostics; escalate
  user-shadows-imported-stdlib to Error; migrate the in-tree corpus with the shared
  `fix --imports` tooling.
- **N4 — qualified references.** Grammar production for `<binding>.Name` in expression and
  type position; resolution over DefEnv's `(module_key, name)` keying
  (resolution-unification I7).

---

## §3 — Contract (H)

### §3.1 Name-environment policy (shared by compile and eval)

- **NS-P1 (precedence).** A bare name resolves: local module first, then imports in
  declaration order, first match wins. (Identical to resolution-unification I6; after N3
  there is no ambient stdlib tier — stdlib modules are ordinary imports.)
- **NS-P2 (collision dispositions).**
  - intra-stdlib pub collision, any name kind → **Error at stdlib build** (panic path,
    naming both modules). Consequence: stdlib registration order is semantically inert.
  - user declaration shadowing an *imported stdlib* name → **Error** (post-N3;
    Severity::Warning in the N0 interim, matching resolution-unification D-6).
  - user-import vs user-import, entry vs user-import → first-wins + Warning naming both
    origins (resolution-unification D-6, unchanged — owned there).
- **NS-P3 (kind uniformity).** NS-P1/P2 apply to every name kind: templates, enums,
  functions (per name+signature), units, aliases, traits, constraint defs, purposes.
  The enum `prelude ++ local` inversion and the template last-wins collects are replaced
  by this rule.
- **NS-P4 (compile/eval parity).** For every fixture, compile-side binding and
  DefEnv/eval-side binding are the same definition. Pinned by a parity test seeded with the
  `Mode` fixture. The policy (ordering + dispositions) lives in one shared implementation
  point consumed by both sides; a prose-only sharing of the rule is a G7
  `contracts-machine-checked` violation.

### §3.2 Strict visibility (post-N3)

- **NS-V1.** Every module (entry or library — no tier distinction) sees: language builtins
  (`Real`, `Int`, `Bool`, `String`, geometry primitives, builtin fns/dimensions) + the pub
  surfaces of its declared imports. Nothing else. `import std.prelude` is an ordinary
  import of a facade module that `pub import`s the curated core (units, si_units,
  option_recovery, result, determinacy.purposes, geometry traits — final list is a
  curation task).
- **NS-V2 (fix-it).** An unresolved name that exists in exactly one stdlib module produces
  a structured fix-it diagnostic naming that module and `std.prelude`; ambiguous matches
  list candidates. (G7 `structured-facts-at-failure`.)
- **NS-V3.** Stdlib modules obey NS-V1 themselves (N2): a stdlib module referencing an
  undeclared sibling fails the stdlib build. Stdlib files gain `module std.<path>` headers;
  the `is_std_path` skip of `attach_module_path_diag` (module_dag.rs:527) is removed.

### §3.3 Qualified references (N4)

- **NS-Q1.** `<binding>.Name` where `<binding>` is an import's binding name (its alias if
  `as` was used, else the final path segment) resolves `Name` in that module's pub surface
  **only** — no fallback to unqualified resolution, no transitive reach-through (re-exports
  via `pub import` are part of the pub surface, per resolution-unification D-7).
- **NS-Q2.** Valid in expression position (ctor calls, fn calls, enum access
  `pp.FitClass.Clearance`) and type position (`param p : pp.Pulley`). Grammar must
  disambiguate from member access; the resolution-phase rewrite mirrors
  resolution-unification D-9's MemberAccess→EnumAccess fixup pattern.
- **NS-Q3.** Implemented over DefEnv's `(module_key, name)` internal keying
  (resolution-unification I7) — no re-plumbing of call sites.

### §3.4 Stdlib delivery (N2)

- **NS-M1.** `std.*` resolves through the ordinary `ModuleDag` walk from an **embedded
  read-only source mount**; per-module compile results are memoized per process (the memo
  is an optimization, never a semantic regime). `StdlibMode`, the partial-overlay
  diagnostics, and implicit `stdlib_root` filesystem probing are deleted.
- **NS-M2.** Error-severity diagnostics in any stdlib module still fail fast (panic) at
  stdlib build — a stdlib error is a compiler bug.
- **NS-M3.** Registration/mount order is unobservable (guaranteed by NS-P2's intra-stdlib
  Error): a determinism test permutes the stdlib source list and asserts identical
  compiled output. Replaces `signal_2_..._order_is_stable` (deliberately retired).

---

## §4 — Resolved design decisions

- **D-1 (intra-stdlib collision = Error).** Leo 2026-07-24. Forces the `Mode` rename in
  the same diff. Rename side: buckling → `BucklingMode` (modal keeps `Mode`, matching what
  check-time user code binds today; buckling's fields were unreachable anyway *(probe)*,
  so the rename's blast radius is stdlib-internal + accessor fns + goldens).
- **D-2 (user-shadows-imported-stdlib = Error, staged).** Leo 2026-07-24. Warning in N0
  (matching resolution-unification D-6 interim), escalated to Error at the N3 flip when
  stdlib names in scope become explicitly chosen.
- **D-3 (strict-by-default everywhere).** Leo 2026-07-25. Ratchet asymmetry: loosening
  later is backward-compatible, tightening later breaks code; the parts-library program
  multiplies ambient collision surface right when tightening would be needed. No
  entry-vs-library tier distinction — one visibility rule.
- **D-4 (`import std.prelude` facade; zero implicit core tier).** Leo 2026-07-25. The
  escape valve is an ordinary curated facade module built on `pub import` re-export
  (resolution-unification μ), not an `#ambient_imports` pragma: one visibility mechanism,
  versionable, user-imitable for project preludes, provenance stays in the file. Nothing
  is ambient beyond true language builtins. GUI scratch buffers inject the prelude import
  textually (UI default, not a language mode).
- **D-5 (stdlib = ordinary modules over an embedded mount).** Leo 2026-07-25. The only
  specialness that survives scrutiny: distribution (embedded mount — single-binary,
  version-pinned-by-construction) and trust (NS-M2 panic). Everything else — prelude
  visibility, privileged unit compilation, the separate resolution regime — is accreted
  and deleted by N2/N3. Build-time precompilation (compiling stdlib during `cargo build`
  like an `.rs` file) was considered and rejected for now: bootstrap circularity forces a
  two-stage build + a stable IR serialization format, churned by every IR change, buying
  unmeasured startup milliseconds (§10 Q4).
- **D-6 (fs stdlib resolution: not "retired" — unrepresentable).** Subsumed by D-5. A dev
  override, if ever wanted, is a remap of the mount (all-or-nothing by construction);
  re-add as fresh work only if a real consumer appears.
- **D-7 (qualified-ref binding form).** Qualification is via the import's binding name
  (alias or final segment), Rust-like — not arbitrary dotted absolute paths. Full-path
  qualification (`std.units.Length` with no import) is out of scope v1 (§9).
- **D-8 (phase-order fix = authoritative templates before fn bodies).** Delete
  `build_structure_def_skeleton` rather than patching its hazards — it is a lockstep
  mirror of the authoritative template builder (G7 `no-lockstep-duplication`, standing
  violation resolved). The two-sub-pass split anticipated in entity.rs:5982-5984 is the
  implementation direction.
- **D-9 (import-header generation is shared tooling).** One tool derives "unresolved name
  → defining module → import list" for both the stdlib conversion (N2) and the user-corpus
  migration + ongoing `fix --imports` (N3). Built once, consumed twice.
- **D-10 (identity-order test retired deliberately).** Replaced by NS-M3 determinism.
  The old test guarded hand-order stability; the new one guards the stronger property
  (order inertness).
- **D-11 (resolution-unification interim superseded knowingly).** Their I4 ("`std.*`
  never DAG-walked") and their β's blanket stdlib seeding of import units are correct for
  their landing window and are superseded by N2/N3 here. Recorded in their §6 (amended in
  this branch) so neither decompose treats it as drift.

---

## §5 — Pre-conditions for activating

- **N0, N1:** none — land against main as-is.
- **N2:** N0 (β's collision-Error must precede declared-imports reordering so a reorder
  can never silently change a binding); N1 (fold order: δ before ζ rewrites headers).
- **N3:** resolution-unification **μ** (`pub import` re-export — `std.prelude` is built on
  it) and their P2 landing (θ/ι/κ — flipping visibility against the unified env, not the
  interim merge path). Real `add_dependency` edges at decompose against their filed ids.
- **N4:** μ (grammar producer, in-batch) and resolution-unification **β** (DefEnv skeleton
  + I7 keying).

---

## §6 — Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `resolution-unification.md` (landed) | both | (a) eval-side collision policy D-6/I6 — theirs; compile-side alignment (N0 α) — ours; parity NS-P4 test — ours. (b) DefEnv I7 `(module_key,name)` keying — theirs; qualified lookup over it (ν) — ours. (c) `pub import` re-export (their μ) — consumed by ι. (d) Their I4 + β interim stdlib-seeding — superseded by κ/η (D-11), amendment recorded in their §6. (e) Their §1 collision-fragmentation note + scope fence — points here. | per-item as listed | amendments to their PRD land in this same branch |
| #5391 standard-parts library (bookmark) | consumes | strict library modules + collision survivability (κ), `std.prelude` + re-export story (ι), qualified refs for part-name disambiguation (ν) | this PRD produces | wire `add_dependency` 5391 → {κ, ι, ν} at decompose |
| #5386 / #5403 exit-code invariant | adjacent | none — this PRD adds ordinary Error diagnostics; exit policy unchanged here | #5403 | no edge |
| `eradicate-silent-undef.md` | aligned | NS-V2 fix-its follow its structured-diagnostic conventions; skeleton-poison deletion (γ) removes a silent-Undef source | each own | reference only |
| `conditional-compilation.md` (landed) | consumes | `#cfg` on `std.*` imports becomes meaningful under strict (gates visibility, same `import_cfg_satisfied` mechanism) | this PRD (κ) | semantics unchanged, one boundary row |
| `module-and-visibility-hardening.md` | extends | stdlib files gain `module` headers; std-skip of `attach_module_path_diag` removed (NS-V3, ζ) | this PRD (ζ) | extension of landed mechanism |
| tree-sitter-reify / lezer grammars | produces | qualified-ref production (μ) in tree-sitter + reify-syntax lowering + GUI lezer highlighting | this PRD (μ) | grammar-gate fixtures committed with μ |

---

## §7 — Boundary-test sketch (H)

| # | Scenario | Preconditions | Postconditions | Phase |
|---|---|---|---|---|
| 1 | `Mode` compile/eval parity | β's rename landed | compile-side and eval-side bind the same definition for `Mode` (→ modal) and `BucklingMode` (→ buckling) | N0 (β) |
| 1a | NS-P4 parity property | any fixture corpus | *for every fixture*, the compile-side binding and the eval-side binding are the SAME definition — both read the one shared policy point (NS-P4). Seeded from #2's user-shadows-prelude fixture and synthetic two-peer prelude sets, **not** from an intra-stdlib collision (β makes those a build Error, so the stdlib has no live collision to witness) | N0 (α) |
| 2 | User enum vs stdlib enum | user `enum SignalKind { Foo }` | `SignalKind.Foo` resolves (local wins); Warning (N0) → Error (post-κ) | N0 (α) / N3 (κ) |
| 3 | Injected duplicate stdlib pub name | test-only stdlib set with two `Mode`s | stdlib build fails naming both modules (negative-assertion: the rejection observably fires) | N0 (β) |
| 4 | `BucklingMode` rename | — | buckling accessors (`critical_load`, `mode_shape`) still resolve + evaluate; `BucklingMode` + modal `Mode` coexist in one fixture. Blast radius is **not** stdlib-internal: the live trampoline hardcodes the struct name at `reify-eval/src/compute_targets/buckling.rs:513,:822,:996` (all above the `#[cfg(test)]` at :1083) and must rename in lockstep | N0 (β) |
| 5 | Same-module silent-divergence fixture | struct default referencing a same-module fn; fn body `Foo()` ctor; nested-ctor-in-let | correct value at eval (no poison), or a loud diagnostic — never silent divergence; `git grep build_structure_def_skeleton` empty | N1 (γ) |
| 6 | Vestigial folds | — | `solve_buckling`/`modal_analysis`/`simulate_trajectory` resolve from parent modules; `*_fns` files gone; stdlib count 47→44 | N1 (δ) |
| 7 | Stdlib strict self-compile | ζ headers landed | a stdlib module referencing an undeclared sibling fails the stdlib build (negative fixture); full stdlib compiles green | N2 (ζ) |
| 8 | Order inertness (NS-M3) | ζ | permuting `stdlib_sources()` order → byte-identical compiled output | N2 (ζ) |
| 9 | Embedded mount unification | η | `import std.units` walks the DAG; `CompiledProgram.imports` lists it (`origin: None`); `git grep StdlibMode` empty; a project with a `stdlib/units.ri` on disk compiles identically to one without | N2 (η) |
| 10 | `mechanism_modal_analysis` fold | ζ (declared imports exist) | resolves from `std.modal.analysis`; file gone | N2 (θ) |
| 11 | `import std.prelude` one-liner | ι | file with only that import uses `mm`, `Length`, `unwrap_or`, `simulation_ready` | N3 (ι) |
| 12 | Strict unresolved fix-it | κ | file using `Length` with no import → Error whose text names `std.units` and `std.prelude` (NS-V2); with the import → clean | N3 (κ) |
| 13 | Corpus migration | λ | examples/ + prj/ compile strict; CI green; `fix --imports` output on a stripped file reproduces the committed headers | N3 (λ) |
| 14 | `#cfg`-gated std import | κ | cfg-unsatisfied `import std.x` ⇒ its names invisible (Error on use), satisfied ⇒ visible | N3 (κ) |
| 15 | Qualified-ref grammar | μ | fixtures `stdlib-namespace-{1,2}.ri` shapes parse, 0 ERROR nodes (`tree-sitter parse --quiet` exit 0) | N4 (μ) |
| 16 | Qualified disambiguation | ν | two imports both exporting `Flange`; `a.Flange` and `b.Flange` construct distinct structs (expression + type position); unqualified `Flange` warns first-wins per NS-P1 | N4 (ν) |
| 17 | Qualified no-fallback | ν | `pp.NotThere` → Error naming pp's module; never falls back to unqualified resolution (negative-assertion) | N4 (ν) |

---

## §8 — Decomposition plan

Labels PRD-relative; real ids at decompose. **Leaf** = no in-batch dependent. Gate-resident
test additions carry drift-guard registrations in the same diff (run-all-classification
manifest, wallclock bounds, nextest partitions) per the overlay rule; new test binaries
bump the THROUGHPUT-COUNTS sentinel in-diff.

**N0 — one collision rule (independent, lands first)**

- **β — Intra-stdlib collision = Error + `BucklingMode` rename** *(intermediate → α, δ, ζ;
  lands first in N0)*. Modules: reify-compiler (stdlib_loader/stdlib_topo panic path),
  stdlib .ri (solver_buckling*, goldens), **reify-eval (`compute_targets/buckling.rs` — the
  live trampoline hardcodes `type_name: "Mode"`)**. Signals: #1, #3
  (negative-assertion), #4. Deps: none.
- **α — Compile-side collision/direction unification** *(intermediate → ε, ν)*. Modules:
  reify-compiler (entities_phase, functions_phase, enums_phase, type_resolution, expr,
  prelude_context). Extract the NS-P1/P3 policy to one shared point; fix the enum
  inversion; align template registries to local-shadows + first-wins-among-peers; add the
  NS-P4 parity property test. Signals: #1a, #2(Warning form). **Deps: β.**

> **Ordering amendment (2026-07-27, esc-5493-2).** α↔β were filed with the edge pointing
> the wrong way. β was annotated `Deps: α`, but neither of β's capabilities
> (`duplicate-pub-name-rejection-fires`, `buckling-rename-blast-radius-stdlib-internal`)
> consumes α's policy point — the duplicate-pub-name scan is standalone, and both of β's
> boundary signals (#3, #4) are deliverable against today's `main`. α, by contrast,
> genuinely *requires* β: `stdlib_loader.rs` registers `solver_buckling.ri` (:113) before
> `modal_analysis.ri` (:158), so flipping the template registries to first-wins rebinds
> bare `Mode` from modal's `{frequency,…}` to buckling's `{eigenvalue,…}` and breaks
> `modal_analysis_fns.ri`'s `result.modes[0].frequency` (`functions_phase.rs:64-69` →
> `expr.rs:6483`) — **inside the stdlib**. The edge is inverted here.
>
> Two facts bound the risk. (1) `Mode` is the *only* intra-stdlib collision: a scan of all
> stdlib modules finds exactly one duplicate `structure def` name and zero duplicate
> `enum def` / `occurrence def` names, so β's single rename makes the corpus collision-free
> and β's Error gate keeps it that way. (2) Consequently, boundary #1 (`Mode` parity) is
> **delivered by β alone** — post-rename both sides bind the sole surviving definition — so
> it moves to β, and α takes the non-vacuous #1a property form instead. Had #1 stayed on α
> it would have passed with α's flip entirely absent. δ and ζ already assumed β-first.

**N1 — phase order (independent of N0)**

- **γ — Authoritative templates before fn bodies; delete the skeleton mirror**
  *(intermediate → δ)*. Modules: reify-compiler (lib.rs orchestration, entities_phase,
  functions_phase, entity.rs). Two-sub-pass split; `build_structure_def_skeleton` deleted;
  both documented hazards become impossible. Signal: #5. G7 note: resolves the standing
  `no-lockstep-duplication` violation.
- **δ — Fold `solver_buckling_fns` / `modal_analysis_fns` / `trajectory_fns`** *(leaf)*.
  Modules: stdlib .ri + stdlib_loader registration. Signal: #6. Deps: γ (and β for the
  buckling file's rename landing first — order at decompose).

**N2 — stdlib as ordinary modules**

- **ε — Import-header generation tooling** *(intermediate → ζ, λ)*. Modules: new tool
  (crate or xtask; decision at task). Compile-against-builtins-only, map unresolved names
  → defining module, emit import lists. Signal: tool output for all stdlib files matches
  the headers ζ commits (#13's second clause is the reuse proof). Deps: α (needs the
  unified lookup to attribute names).
- **ζ — Stdlib declared imports + strict per-module compile + module headers**
  *(intermediate → η, θ)*. Modules: all stdlib .ri, stdlib_loader, stdlib_topo. Ordering
  comments deleted; identity test → NS-M3 determinism test (D-10); `module std.*` headers;
  std-skip of module-path diag removed. Signals: #7, #8. Deps: β, γ, δ, ε.
- **η — Embedded mount: `std.*` through ModuleDag; delete StdlibMode** *(leaf)*. Modules:
  reify-compiler (module_dag, stdlib_loader), reify-cli, gui (resolver construction
  sites). Per-module memo cache. Signal: #9. Deps: ζ; sequence after
  resolution-unification β lands (its `compile_program` is the surface that lists the
  units).
- **θ — Fold `modal_mechanism_fns`** *(leaf)*. Modules: stdlib .ri. Signal: #10. Deps: ζ.

**N3 — strict-by-default (gated on resolution-unification P2 + μ)**

- **ι — `std.prelude` facade module** *(intermediate → κ)*. Modules: stdlib .ri +
  registration. Curation list per D-4 (finalize in-task, §10 Q1). Signal: #11. Deps: ζ;
  cross-PRD: resolution-unification μ.
- **κ — Strict-visibility flip + NS-V2 fix-its + shadow-Error escalation** *(intermediate
  → λ)*. Modules: reify-compiler (prelude seeding paths, diagnostics), reify-cli, gui,
  reify-lsp. Supersedes resolution-unification I4 + β-interim seeding (D-11). Signals:
  #12, #14, #2(Error form). Deps: ι; cross-PRD: resolution-unification P2 (θ/ι/κ).
- **λ — `fix --imports` + corpus migration** *(leaf)*. Modules: reify-cli (or the ε tool's
  surface), examples/, prj/, gui templates. Signal: #13. Deps: ε, κ.

**N4 — qualified references**

- **μ — Grammar production: qualified refs (expression + type position)** *(intermediate
  → ν)*. Modules: tree-sitter-reify, reify-syntax (ts_parser lowering), gui lezer grammar.
  `grammar_confirmed: false` — this IS the grammar producer; fixtures committed. Signal:
  #15. Deps: none (parallel to N0-N3).
- **ν — Qualified resolution over DefEnv keying** *(leaf)*. Modules: reify-compiler
  (resolution fixup per NS-Q2), reify-eval (DefEnv qualified lookup). Signals: #16, #17.
  Deps: μ, α; cross-PRD: resolution-unification β (I7 hook).

**Companion corrections at decompose:** wire `add_dependency` 5391 → {κ, ι, ν}; wire the
cross-PRD edges in §5 against resolution-unification's filed task ids; confirm the
amendments to resolution-unification.md (landed in this branch) are reflected in its
decompose (its β/θ carry the interim-seeding + units-flip-compile-side clauses).

---

## §9 — Out of scope

- **Absolute-path qualification without an import** (`std.units.Length` bare) — D-7; v2
  if demand appears.
- **Selective-import narrowing and `pub import` semantics** — resolution-unification P3
  (its ν, μ). This PRD consumes, never reimplements.
- **Exit-code policy** — #5403.
- **Build-time stdlib precompilation** — D-5 rejection; §10 Q4.
- **Catalog/table mechanism, parts content** — #5391's own PRD.
- **LSP auto-import code actions** — natural follow-up to NS-V2's structured fix-its;
  file when the LSP session picks it up (the diagnostic payload is designed to carry the
  needed fields).

## §10 — Open questions (tactical)

1. **`std.prelude` curation list.** Baseline: units, si_units, option_recovery, result,
   determinacy.purposes, geometry traits, io. Decide in ι against dogfood-file usage
   frequency (grep the corpus).
2. **Migration sweep breadth (λ).** examples/ + prj/ certain; the printer-dogfood
   worktree and any user files outside the repo get the tool, not the sweep. Decide in λ.
3. **`import std` (bare) under strict.** Disallow with a fix-it pointing at
   `std.prelude`, or alias it to the facade? Suggested: disallow (explicit is the point).
   Decide in κ.
4. **Build-time stdlib precompilation trigger.** If startup profiling ever attributes
   user-noticeable latency to the per-process stdlib compile, revisit D-5's rejection
   (two-stage build + IR serialization). Requires a measurement first.
5. **ε tool packaging.** xtask vs `reify` subcommand vs standalone script — λ needs it
   user-facing (`fix --imports`), which suggests a `reify` subcommand backed by a shared
   lib fn. Decide in ε.
