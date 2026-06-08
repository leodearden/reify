# Capability manifest — `tolerancing-gdt-surface-completion`

Mechanizes G3 + G6 per leaf for the PRD `docs/prds/v0_6/tolerancing-gdt-surface-completion.md`.
Each row binds a leaf's asserted capability to on-disk evidence. **Any FAIL binding blocks the
batch from being queued.** Evidence forms per the reify `/prd` overlay (anti-orphan/wired,
field-population, grammar-fixture, numeric-floor).

Verification performed at authoring time (2026-06-03) against the working tree; commit hashes
are not pinned because the substrate is pre-existing prelude/resolver code.

| Leaf | Asserted capability | Evidence form | Evidence (file:line / fixture / cell) | Verdict |
|---|---|---|---|---|
| **α** | `iso_it_tolerance`/`effective_tolerance_zone` are **wired** into the builtin dispatch, not orphaned | wired-on-main (anti-orphan) | New `tolerancing::eval_tolerancing` arm added to the `if let Some(v) = …` chain in `reify-stdlib/src/lib.rs` (sibling of `stackup::eval_stackup`, `lib.rs:164`); α's signal greps for the arm | **PASS** (seam exists; arm is α's deliverable) |
| **α** | `iso_it_tolerance` reproduces the ISO 286-1 IT table within rounding | numeric-floor (G6) | Hand-checked cells: IT6@Ø18–30=13µm (calc 13.07), IT7@Ø30–50=25µm (24.97), IT8@Ø6–10=22µm (22.5). Floor = ISO 286-1 published values; envelope IT5–IT18 ≤500 mm, `Undef` outside. α asserts `computed ≈ published` within rounding | **PASS** (premise validated by hand at authoring) |
| **α** | `effective_tolerance_zone` branches on the `MaterialCondition` enum at eval | grammar/substrate | Enum-variant branch precedent: `reify-eval/src/compute_targets/buckling.rs` matches `Value::Enum { variant, .. }` on `"P2"` | **PASS** |
| **β** | `nominal_zone` derived let on a **trait** parses + compiles + produces a non-`Undef` value (field-population) | field-population + grammar-fixture | Precedent `trait Physical { let mass = … }` (`structural_physical.ri:40`); fixture `/tmp/prd-gate-fixtures/np-1.ri` → `reify check` exit 0. Non-`Undef` requires α's builtin (β deps α) — without α the let evaluates to `Undef` (declared-only), so **β must depend on α** | **PASS** (gated on α dep) |
| **β** | `ISOToleranceGrade.tolerance_value` derived let calling the builtin | field-population + grammar-fixture | fixture `np-1.ri` compiles; value is non-`Undef` only after α (β deps α) | **PASS** (gated on α dep) |
| **β** | `Conforms` trait-typed param + member-access predicate compiles + evaluates | grammar-fixture | `compile_constraint_def` accepts trait/structure/enum params (`defs_phase.rs:88-112`); member-access-in-constraint precedent `constraint material.density > 0` (`structural_physical.ri:44`); fixture `np-2.ri` → `reify check` exit 0 | **PASS** |
| **β** | `require_finish` `.ri` fn evaluates to a real `Bool` (not `Undef`) | field-population (eval-proof) | fixture `/tmp/prd-gate-fixtures/req-clean.ri` → `reify eval` prints `UseIt.ok = true`; `.ri` fns need no Rust registration (`eval_user_function_call`, `reify-expr/lib.rs:1064`) | **PASS** (proven by eval) |
| **β** | `SurfaceFinish.process` default uses `""` (NOT `= undef`) | grammar/substrate (negative finding) | `param … = undef` **fails on structure params** (`u-a.ri`/`u-b.ri` → `error: unresolved name: undef`), works only on traits (`u-c.ri` exit 0). `String = ""` default compiles (`req-clean.ri` exit 0) | **PASS** (sentinel chosen; doc-reconcile not grammar-work) |
| **γ** | `symmetric_tolerance`/`limit_tolerance` return a constructed `DimensionalTolerance`; `Fit` nested members | grammar-fixture + field-population | `DimensionalTolerance` already declared (`tolerancing.ri:30`); structure-ctor with named args + member access proven (`req-clean.ri`). γ's signal reads `.upper_limit` off the return | **PASS** |
| **ε** | the §7 example runs green in CI (the user-observable leaf) | wired-on-main (integration gate) | `reify eval examples/tolerancing/std_tolerancing_surface.ri` in CI; prints IT widths / expanded zones / MMC-vs-RFS flip. Depends β+γ so all producers are landed | **PASS** (gate task) |
| **δ** | `param feature : Geometry` resolves | grammar/substrate | Today `unresolved type: Geometry`; `Solid` already → `Type::Geometry` (`type_resolution.rs:563`). δ adds the `"Geometry"`/`"DatumRef"` arms (≈1 line each) | **PASS** (lift confirmed trivial; cascade is the real work, scoped in δ) |
| **δ** | flipping `feature : Real = 0.0` → `feature : Geometry` invalidates the default | premise-honesty | `Geometry` has no literal default → `feature` becomes **required** → cascade to examples/tests. δ's signal includes `cargo test -p reify-compiler` green after migrating all sites | **PASS** (cascade named, not silently assumed) |

## Anti-orphan / anti-inversion summary

- **No orphan producer:** α's builtins are consumed by β's `nominal_zone`/`ISOToleranceGrade`
  lets and `Conforms` predicate — all in the same batch, dependency-wired (β deps α). The §0
  separation guarantees they are NOT consumed by (or coupled to) the kernel-budget or stackup
  subsystems.
- **No field-population inversion:** β's derived lets are non-`Undef` only after α lands; the
  β-deps-α edge prevents a "declared-only" landing. ε (the CI gate) reads real values, so a
  silently-`Undef` let fails the gate.
- **No grammar fiction:** every novel fragment parsed under `tree-sitter` AND compiled under
  `reify check`; the one negative finding (`= undef` on structures) is resolved by a sentinel,
  not deferred to grammar work.
- **No false numeric premise:** the ISO 286-1 floor cells were hand-computed at authoring and
  match published values; the supported envelope (IT5–IT18, ≤500 mm) is stated, with `Undef`
  outside.
