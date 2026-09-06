> **Provenance.** Relocated evidence artifact, verbatim. Produced by the 2026-08-03
> fallback-soundness investigation; it is the evidence base cited by
> `docs/prds/v0_6/builtin-signature-registry.md`. Everything below was measured against main
> `36738b9b92`. Companion raw data: `fallback-soundness-xref-2026-08-03.json` (the two flat name
> sets the set-diff was taken over — `eval`: 231 names, `fallback`: 121 names — plus a
> `corrections` array added 2026-08-25; see Corrections below). Moved into the
> repo on 2026-08-07 because the originally-cited session scratchpad path no longer exists.
> This is a dated snapshot, not a maintained document.

> **Corrections (2026-08-07 review).** A follow-up review (the type-decision enshrinement
> review, ratified 2026-08-07) re-probed this snapshot's load-bearing claims. Four passages
> below would actively mislead and carry inline `[<date> correction]` markers at the
> affected spots:
>
> 1. `complex_mul`/`complex_div`/`complex_pow` were classified FALLBACK-CORRECT by a
>    Value-kind-only check; `Type::Complex` carries a quantity parameter (ty.rs:199) which
>    these transform (A·B, A/B, A^n — complex.rs:152/:179/:191-248; probed: `complex_mul` of
>    two `Complex<m>` values → m²). They are mistyped today. `complex_sqrt` is correct only
>    per the explicit "for v0.6" dimensionless deferral (complex.rs:271); `mod` is correct
>    only on the Int×Int subdomain (every other arg shape is statically fallback-typed but
>    eval-Undef). The FALLBACK-CORRECT/WRONG split shifts 16→13 / 93→96 accordingly;
>    headline counts elsewhere in this snapshot are left as written.
> 2. The prose "project(point, plane)" is wrong: `project`'s arg1 is decoded exclusively as
>    `Value::Frame` (geometry.rs:962-964); a plane evals Undef. (Classification unaffected.)
> 3. The blanket "typed Scalar<Length>/Real" for the `prb_*` family is imprecise for
>    `prb_cartwheel_flexure`, which leads with an Int `blade_count` (flexures/compound.rs:289)
>    and is fallback-typed `Int`.
> 4. [2026-08-25 correction: `iso_it_tolerance`, listed in the FALLBACK-WRONG Stackup/DFM/tolerancing… bullet below
>    (`-> Scalar<LENGTH>, typed Int`), has moved to FALLBACK-CORRECT. Task #6091 (RULING,
>    merged `12d6b19353`, on `main` as of 2026-09-03) flipped `iso_it_tolerance`'s argument
>    order from grade-first `(grade, nominal_min, nominal_max)` to subject-first
>    `(nominal_min, nominal_max, grade)`. The name is still not registered in any
>    compiler-ladder name family (units.rs / signature files) either before or after the
>    flip, so its call sites still fall through `NoUserFunctions` to the terminal
>    first-arg fallback — but the new arg0 (`nominal_min`, a `Scalar<LENGTH>`) now matches
>    the true result type, where the old arg0 (`grade`, an `Int`) did not. Measured
>    first-hand on branch task/6091 (assertion `(a2)` in
>    `iso_tolerance_grade_tolerance_value_derived_let`,
>    crates/reify-compiler/tests/tolerancing_tests.rs): mutating the prelude call site
>    (crates/reify-compiler/stdlib/tolerancing.ri) back to grade-first reproduces
>    `left: Int, right: Scalar { dimension: LENGTH }`; subject-first infers `Scalar<LENGTH>`
>    correctly. This is an untracked side effect of the arg-order ruling, not a deliberate
>    registry fix — registry τ4 (task #6006) is still `pending` as of 2026-09-03 and has not
>    added an explicit `iso_it_tolerance` row; when it lands, the name moves again, from
>    FALLBACK-CORRECT to EXPLICIT, and stops depending on the fallback ladder at all.
>    Headline counts (FALLBACK-WRONG 93 / FALLBACK-CORRECT 16) elsewhere in this snapshot are
>    left as written, per the same convention as correction 1. Companion raw-data file
>    `fallback-soundness-xref-2026-08-03.json` is unaffected by this correction: its
>    `fallback` list is flat ladder-fallthrough membership, not a WRONG/CORRECT split, and
>    `iso_it_tolerance` genuinely still falls through to the ladder — see that file's own
>    `corrections` key for the parallel note.]

---

# The first-arg-type fallback: soundness analysis and recommendation

Investigation session 2026-08-03, agent team of 5 (ladder anatomy, enumeration, in-flight interactions, anti-pattern sweep, detector feasibility). All file:line refs against main 36738b9b92. Claim labels: [verified] = read/probed by an agent or me this session; [hypothesis] = reasoned, not executed; [not-checked] = flagged, unexamined.

## Verdict on the thesis (TLDR)

The thesis is CONFIRMED and understated. Measured: of 231 eval-dispatchable builtins, 121 fall to the fallback and **93 are typed wrongly** — the fallback is wrong for 77% of its actual clientele, not an edge case; only 16 names are genuinely served by it. The wrong set includes silently wrong VALUES, not just types (floor(2.5mm) → Int(0)). The project has already ratified — but not implemented — the durable fix: INV-COMP-2's end-state "type (unified registry, Wave 3)" = bookmark task 5068, whose own text specifies a table including result types. Closing the fallback is right; it is a rollout problem (registered clientele + sequencing traps), not a design problem. And it is not a green field — tasks 5371, 5979, 5380, 5344, 5068 already tile the fix space; the work should land through them, not as new duplicates.

## Q1 — The issue in detail

### What the fallback is and where it sits [verified]

Path: `ExprKind::FunctionCall` (expr.rs:2164) → pre-intercepts (auto-gate, determinacy scope gate, `some`, `filter`, `cost`, `generate`, structure-ctor lowering) → `resolve_function_overload(name, args, functions)` over user + prelude/stdlib `.ri` functions (type_compat.rs:1155). If no same-named user/stdlib fn exists → `OverloadResolution::NoUserFunctions` (expr.rs:3066).

There is NO name-existence gate anywhere on this path [verified]. The compiler fabricates `ResolvedFunction { qualified_name: format!("std::{}", name) }` unconditionally for any bareword (expr.rs:3204-3207) — the "resolution" is a string format, not a lookup. Contrast: `TraitStaticCall` DOES hard-error on unknown names (expr.rs:6059-6089), so the machinery exists — it just isn't applied to bareword calls.

The result type is then computed by a 21-arm if/else-if ladder (expr.rs:3210-3585): 20 registered name-family arms (geometry query helpers, kinematic queries, ~30 topology selectors, relations, datum constructors, geometry queries, selector composition, tolerancing, 65 geometry constructors, list helpers, dynamics, affine maps ×2, math signatures, joints, analysis, FEA envelopes, field ops, parse fns) and the terminal else at expr.rs:3568-3584:

- ≥1 arg → adopt `compiled_args.first().result_type` with ZERO diagnostics;
- 0 args → `Severity::Warning` "cannot infer return type of zero-arg function" + `Type::dimensionless_scalar()` (the only diagnostic the fallback ever emits).

Family arms' name lists live in units.rs consts + six signature files (builtin/math/joint/analysis/parse/relation_signatures.rs); pairwise disjointness is pinned by 32 `*_are_disjoint*` tests so ladder order is unobservable [verified].

### What it is there for [verified origin]

The fallback is an M1 fossil: commit a32a1b2b2e (2026-03-16) introduced it as "For math functions, use the type of the first argument as a heuristic". The math family has since been carved out (tasks 4179/4182/4352 → math_signatures.rs), so the original clientele is gone. Its remaining legitimate work:

1. ~15 type-preserving builtins still correctly typed by first-arg propagation: `transform_compose`, `transform_inverse`, `orient_compose`, `orient_inverse`, `orient_slerp`, `project(point,plane)`, `complex_add/mul/div/pow/sqrt/exp` [verified vs eval arms]. *[2026-08-07 correction: `complex_mul/div/pow` are not type-preserving — they transform the `Complex` quantity parameter; and `project`'s second arg is a Frame, not a plane (a plane evals Undef). See Corrections at top.]*
2. A deliberate graceful-degradation escape hatch: three arg-aware arms (list helpers, affine algebra, field ops) return `None` on mis-shaped args and fall to the fallback for a "zero regression guarantee" (documented at expr.rs:3530-3533) [verified].
3. Eval-deferred names in single-file compile regimes: today `reify eval`/`build` compile single-file, so calls to *imported user fns* find no user fn and ride the fallback to eval (see Q5 sequencing trap) [verified from RU PRD boundary #10].

### Precisely when it is wrong vs imprecise [verified]

Wrong exactly when `result_type(f) ≠ result_type(arg0)`. Three regimes:

- (a) Eval-implemented, compiler-unregistered builtin: static type lies, runtime value is real → silent wrong static type (frame_to_frame class). Worst sub-case: eval computes on the SI-erased f64 (`Value::as_f64` folds Scalar→f64), so the VALUE is wrong too — probed: `floor(2.5mm)` statically `Scalar<LENGTH>`, evals `Int(0)` (floor of 0.0025) with exit 0 [verified probe].
- (b) Completely nonexistent name, ≥1 arg: zero diagnostics; eval's terminal returns bare `Value::Undef`, which `value_type_kind_matches` accepts for ANY type, and strict Undef propagation spreads it — a typo becomes a quietly-undetermined cell (task 5371's finding, re-verified).
- (c) Zero-arg unknown name: warning + Real — wrong for `plane_xy`, `axis_x`, `frame3_identity`, `orient_identity` etc., but at least loud-ish.

### Why the lie matters downstream [verified consumer paths]

1. It persists: `cell_type = compiled_expr.result_type` (entity.rs:2356 et al.).
2. False overload failures on VALID code: `let t = frame_to_frame(a,b)` types Frame(3); passing `t` to `fn f(t: Transform3)` → exact-equality overload match fails → hard "no matching overload" error.
3. MemberAccess routes to the wrong handler (the ε1 exhaustive match dispatches on the lying type) — Transform members unreachable on a Frame-typed cell.
4. Runtime param-override validation rejects GUI edits on mistyped cells (`TypeKindMismatch` in engine_admin.rs:119-124).
5. LSP hover/completion display the lie.
6. The `let`-annotation checker was deliberately WEAKENED (gated to Int|Scalar, entity.rs:556) partly because builtin result types lie — the fallback is already distorting adjacent design.
7. NOT affected: content_hash (result_type not hashed) [verified]; and `value_type_kind_matches` runs only on param-override/admin paths, never cold-start eval (joint_signatures.rs:15-28) — so on the main eval path every wrong static type is fully silent [verified].

## Q2 — Measured enumeration (main @ 36738b9b92, 2026-08-03)

### Method [verified]

Eval surface: every top-level match arm of the 25 sub-dispatchers chained by `reify_stdlib::eval_builtin` (stdlib lib.rs:225-302), extracted by a brace-balanced script and hand-verified for irregular dispatchers, PLUS the reify-expr native interceptors that fire before the stdlib fallthrough (reify-expr lib.rs:283-556). reify-eval's engine post-processors dispatch only compiler-family names (pinned by registry_drift_tests.rs — reused, not re-derived). Compiler surface: every family slice feeding the expr.rs:3066-3604 ladder, transcribed from units.rs + the six signature files + list_helpers + determinacy predicates. Cross-reference: mechanical set-diff (script + xref.json in session scratchpad); per-fallback name, the eval arm was read to determine the actual returned Value constructor. Reachability: every name unresolved as a user fn compiles through this ladder, so ALL names are .ri-callable in principle; corpus-attestation checked against examples/*.ri, docs/reify-stdlib-reference.md, and stdlib .ri bodies. Snapshot caveat: task 5344 (in-progress) will register ~18 orientation/frame/transform ctor names — the orientation/frame rows below shrink when it lands.

### Totals

| Class | Count |
|---|---|
| Total eval-dispatchable builtin names | **231** |
| EXPLICIT (compiler ladder arm) | 110 |
| FALLBACK total | **121** |
| — FALLBACK-WRONG | **93** |
| — FALLBACK-CORRECT | 16 |
| — FALLBACK-VARIES | 5 |
| — SHADOWED (typed stdlib .ri decl owns user call sites) | 7 |
| UNCLASSIFIED | 0 (5 rows carry [hypothesis] on the precise return shape; the ≠-first-arg conclusion is verified for all 93) |

The fallback is wrong for **93 of its 121 actual clients (77%)**. The five suspected names were confirmed except vec3 (and vec2), which are now EXPLICIT via MATH_CONSTRUCTION_NAMES — Mem0 f784f2a7 is stale on that point; point3/point2 remain fallback-wrong.

### FALLBACK-WRONG (93), grouped; full per-name table with eval file:line in the enumeration agent report + xref.json

- Frames/transforms/datums (18, geometry.rs): frame3, frame3_identity, transform3, transform3_identity, frame_to_frame (→Transform, typed Frame), transform_exp, transform_log (→Map, typed Transform), plane_xy/xz/yz, axis_x/y/z (zero-arg → warning+Real), bbox, bbox_size, bbox_center, point2, point3 (→Point, typed Scalar<D> of first component; point3 attested in 35 example files).
- Orientation (10, orientation.rs; shrinks under 5344): orient_identity, orient_quaternion, orient_euler, orient_basis, orient_look_at, orient_axis_angle, orient_exp (→Orientation), orient_log (→Vector), orient_to_euler (→List of Angles), orient_to_axis_angle (→Map).
- Numeric/complex (5): floor/ceil/round (→**Int of the erased SI value** — silently wrong VALUE for dimensioned args; probed), re/im (→Real, typed Complex).
- Joints (5, joints.rs): transform_at (→Transform, typed joint StructureRef; attested in 4 examples), joint_axis, joint_range, joint_ratio, joint_offset.
- Flexures (14): all prb_* constructors return joint Maps, typed Scalar<Length>/Real; several example-attested. *[2026-08-07 correction: imprecise for `prb_cartwheel_flexure`, which leads with an Int `blade_count` (flexures/compound.rs:289) and is fallback-typed `Int`. See Corrections at top.]*
- FEA (11, fea.rs): envelope_max/min (→Field, typed Map<String,Field>), case_names, result_for, linear_combine, min_max_stress, worst_case (→String; real impl in reify-expr), worst_buckling_case, envelope_critical_load, envelope_argmax/argmin.
- Mechanism/snapshot/dynamics (7): world, bodies, transform_of, sweep_grid, ramp_profile_lower, inverse_dynamics_lower, inverse_dynamics_at_snapshot_lower.
- Stackup/DFM/tolerancing/supports/loads/tensegrity (12): contributor, contributor_asym, stackup_worst_case, stackup_rss, monte_carlo_stackup, fits_build_volume (→Bool, typed BoundingBox), iso_it_tolerance (→Scalar<LENGTH>, typed Int), DisplacementSupport, RollerSupport, gravity, tensegrity_wires, tensegrity_surfaces. *[2026-08-25 correction: `iso_it_tolerance` is now FALLBACK-CORRECT under the #6091 subject-first arg order. See Corrections at top.]*
- Trajectory internals (9): gcode_import_lower, end_effector_track_at, deviation_from_nominal_at, peak_deviation_at, evaluate_profile_at/_dot_at/_ddot_at, profile_duration_at, piecewise_polynomial (eval is a permanent Undef stub — fallback typing is vacuous).
- reify-expr native (2): argmax/argmin (→domain coordinate Point, typed Field).

### FALLBACK-CORRECT (16) — what a hard closure would regress without prior registration

transform_inverse, transform_compose, project, orient_inverse, orient_compose, orient_slerp, input_shape_apply, to_global, effective_tolerance_zone, mod, complex_add/mul/div/pow/exp/sqrt. (Caveat: the orient_* trio is only correct when arg0 wasn't already mistyped by an unregistered orient ctor upstream.) *[2026-08-07 correction: `complex_mul/div/pow` are misclassified here — mistyped today via the `Complex` quantity parameter; `complex_sqrt` is correct only per the explicit "for v0.6" dimensionless deferral (complex.rs:271); `mod` is correct only on the Int×Int subdomain (other shapes statically fallback-typed but eval-Undef). See Corrections at top.]*

### FALLBACK-VARIES (5)

sinh/cosh/tanh/log10: Real of the erased SI value — correct for dimensionless args, wrong in kind AND value for dimensioned args. remap: correct iff from-dimension == to-dimension.

### Three-surface disagreements (LSP vs eval vs compiler) [verified set-diffs]

- LSP documents **35 names the compiler cannot type** (all fall to the fallback) — the IDE advertises `frame3(...) -> Frame3` while the type checker silently contradicts it. Zero LSP names are absent from both other surfaces; ~134 of 231 eval names are absent from the LSP (curated subset, not authority).
- `compose`: in compiler FIELD_OP_NAMES + a typed fields.ri decl, but **no eval arm anywhere** — with std.fields out of scope the compiler types it Field<A,C> and eval yields Undef.
- Name collision `sweep`: the mechanism-sweep builtin (→List<Snapshot>) is claimed by the geometry-constructor arm, which types any value-position `sweep(...)` as dimensionless Scalar — EXPLICIT but wrong, a reminder that explicit-but-placeholder arms (arm 9's `dimensionless_scalar()` for all 66 geometry names in value position) are their own sub-class of the same disease.
- math_signatures.rs:90-92 ADMITS the sinh/floor-class deferral in prose with no task cite — invisible to the PTODO gate [verified].

## Q3 — Candidate fixes

All options assume the ~15 type-preserving names and the 5380 open-ruling names get explicit registrations or ledger entries first — no option survives without that.

**O1. Fail loudly (terminal diagnostic).** Terminal else → `E_UnresolvedFunction`-style diagnostic + `Type::Error` poison (anti-cascade, BareScalarType pattern). Lands: expr.rs terminal arm + a new closed-world membership surface (task 5371 already sketches `unresolved_function.rs` with `EVAL_DEFERRED_BUILTIN_NAMES` + `is_known_builtin`). Per Leo's ratified rollout posture: warn-mode corpus sweep → fix producers → flip to error with break-glass knob.

**O2. Mandatory registration + exhaustiveness.** Strings can't be rustc-exhaustive; real form = closed-world manifest + CI test drawing candidates from the EVAL side (precisely the landed drift test's declared blind spot). Becomes "type"-enforced only if builtin names become registry-generated IDs — which is O3.

**O3. Single shared source of truth (the Wave-3 registry, bookmark 5068).** One metadata table: name, arity, arg slots, result type, eval dispatch target; compiler ladder + eval dispatch + LSP table all derive from it; migrated family-by-family with the landed drift test as migration validator (5068's own text). Registration drift becomes structurally impossible: an eval arm without a row is unreachable, a row without an eval target doesn't build, the compiler cannot type a name the table doesn't know.
Honest limit: the table pins the DECLARED result type; a buggy eval fn body can still return a different Value kind. That residue needs an executed static-vs-runtime parity harness DRIVEN BY THE SAME TABLE (name, sample args, expected type → `value_type_kind_matches`) — single source, no second derivation. Full "structurally impossible" for the value kind would require typed per-family eval signatures — likely too invasive for the Value-based interpreter [hypothesis].

**O4. Keep fallback, narrow to allowlist.** `FALLBACK_TYPE_PRESERVING_NAMES` family + disjointness test; terminal else fires only for members; others → diagnostic. This is O1 with the clientele registered — same thing in practice; the allowlist is step 1 of O1's rollout.

**O5 (team-found). Registry-independent adjacencies:** (i) the three arg-aware arms' None-fallthrough should emit their OWN mis-shaped-arg diagnostics instead of riding the fallback; (ii) the unresolved-name diagnostic should live where DefEnv resolution lands so it survives resolution-unification [hypothesis].

## Q4 — Trade-offs and recommendation

Judged per Leo's directive (architectural quality, long-term performance/maintainability over expediency):

- O1/O4 alone: cheap, kills the bug class at the compile boundary, but leaves 8 signature files + eval maps + LSP table as parallel registries — drift pressure persists; every new builtin still needs N-place registration (the "hand-registering one escalation at a time" failure mode continues, just louder).
- O2 alone: converts silent drift to CI-loud drift; still two derivations.
- O3: the only option making the divergence structurally impossible (for membership + declared signature) rather than detected. Cost: a real PRD + family-by-family migration across 8 compiler files, eval maps, LSP table, subsuming 32 disjointness tests. De-risked: the migration validator (registry_drift_tests.rs) is already landed and 5068 already names the plan; the per-family signature-file idiom means migration is incremental, not big-bang. In a year, O3 leaves ONE place to add a builtin, the ladder becomes a table lookup + small arg-aware residue, and 5922/5704/5707's LSP parity derives instead of being hand-checked.
- Recommended: BOTH, sequenced. O1/O4 as the interim (it is also O3's precondition: the corpus sweep that enumerates and registers stragglers is exactly the registry's seed data), then O3 as the durable end-state. This matches INV-COMP-2's already-ratified "test interim → type" trajectory — the investigation's conclusion is the invariant registry's existing plan, with the addition that RESULT TYPES (not just names) must be in the table, which 5068's text already specifies.

## Q5 — Interactions with in-flight work (sequencing)

[verified unless noted]

- **Task 5344 (in-progress NOW, live branch):** registering ~18 orientation/transform/frame constructor names in the exact ladder region. Highest textual-conflict surface. Land it first; it shrinks the wrong set and follows the template.
- **Task 5436 (pending high, dispatchable):** registers its new `in_frame` explicitly (units.rs arm + signature rows) — the per-family template; canonical arm to copy is `datum_constructor_result_type` (units.rs:644, wired expr.rs:3264). Correction: the frame_to_frame follow-up was filed from esc-5436-2 (recorded on task 5979), not esc-5436-4 [verified from 5979's record; an esc-5436-4 may exist separately — not-checked].
- **Task 5979 (pending low):** register frame_to_frame → Transform(3). Subsumed by any chosen option — fold in, don't duplicate.
- **Task 5371 (pending low):** IS option O1, framed as a design question, with the closed-world manifest sketched. The fallback closure should land AS PART OF a rewritten 5371 (expand-scope-means-rewrite) or explicitly supersede it.
- **Task 5380 (pending low):** the known-fallthrough inventory with OPEN design rulings (BoundingBox quantity slot; heterogeneous Map returns for orient_to_axis_angle/transform_log). A hard fallback closure cannot land before these names are ruled or exemption-ledgered.
- **Task 5068 (deferred BOOKMARK):** the Wave-3 registry slot — activate via /prd to do O3; doing O3 ad hoc outside it orphans the programme's plan.
- **stdlib-namespace (5493 α, reopened; 5495 in-progress; κ=5503 pending):** name→DEFINITION binding policy (name_env.rs), not signatures — orthogonal conceptually, moderate expr.rs textual conflict. One real convergence: κ's strict-visibility flip needs a closed-world builtin-membership authority; closing the fallback BEFORE κ supplies it and keeps κ non-vacuous for calls [hypothesis on κ's exact text].
- **resolution-unification (only α done; compile_program/DefEnv NOT landed — zero grep hits):** SEQUENCING TRAP: today `reify eval`/`build` compile single-file, so imported-user-fn calls legitimately ride the fallback to eval. A hard error before RU γ/δ (5517/5518) breaks importing programs in those commands and invalidates RU λ's pinned pre-state (boundary #10). Mitigations: land the Error flip after RU γ/δ; or warn-severity until then; or scope by compile regime.
- **5867/5970:** same defect CLASS (silent-wrong-type), different seam (prelude-typed sub member typing; orchestrator-infra gate). No code overlap, no ordering constraint; not subsumed by any option here. (5867 has a green implementation branch stuck behind a reviewer turn-cap — 5970.)
- Ordering summary: 5344 land → [warn-mode sweep + register stragglers incl. 5979/5380 rulings] → RU γ/δ → flip terminal to Error (rewritten 5371) → κ benefits → activate 5068 (Wave-3 registry PRD) → registry migration family-by-family → LSP parity (5922/5704/5707) derives from it.

## Q6 — Other instances of the broad anti-pattern

Existing taxonomy (reuse, don't invent): silent-wrong-type, silent-defaults classes; INV-SF-1..7 (docs/legibility/design-invariants.md); INV-COMP-1..3 (docs/invariants.md). Note the anchor bug is best read as an INV-COMP-1 violation ("no type information silently discarded — every fallthrough yields a correct type or a diagnostic"): that invariant is marked enforced, but its enforcement wave only reshaped MemberAccess; the FunctionCall ladder terminal was never covered.

New/undertracked instances found [verified unless noted]:
- **(A) UNTRACKED, HIGH:** un-annotated user-fn return types default to Real (functions.rs:277, :679; traits.rs:153) and NOTHING reconciles declared/defaulted signature vs compiled body type. Probed: silently defeats the 4490 comparison guard → permanently-indeterminate constraint, `reify check` exit 0. Not registry-killable — needs body-vs-signature reconciliation or return inference.
- **(B) folded into Q2:** the floor/sinh math-builtin class (registry-killable).
- **(C) tracked-partial:** compiler vs eval overload resolution are two independent algorithms (type_compat.rs:1195 four disjuncts vs reify-expr lib.rs:1533 three); drift → bare Undef. Task 5686 covers one disjunct; no structural parity test pins the rest.
- **(D) low:** recursion-depth limit returns bare `Value::Undef`, no UndefCause (INV-SF-1).
- **(E) landmine:** unknown type-param default name silently becomes `Type::StructureRef(name)` (type_resolution.rs:3747) — latent (defaults unused downstream today), violates the ratified 4645 principle when generics consumption lands.
- **(F) medium, adjudicated-KEEP with unowned residue:** port/sub member access in expression position: static Real, runtime causeless Undef — statically AND dynamically silent (expr.rs:6668, comment :6585).
- **(H) theoretical:** `wrap_scalar_coord` comment-only contract (field_reductions.rs:1319).
- **(I) from the enumeration:** two EXPLICIT-but-still-wrong sub-shapes — the geometry-constructor arm's `dimensionless_scalar()` placeholder mistypes value-position calls for all 66 geometry names (bites the `sweep` name collision today), and `compose` is compiler-typed with no eval arm anywhere (reverse drift: static says Field, runtime says Undef). Registration alone doesn't guarantee truth — the registry must carry CORRECT result types and be cross-checked against eval.
- Confirmed pinned (do NOT refile): Value::infer_type empty-collection defaults; family-gated `_ =>` arms behind is_* gates; arm_member_type (2846 closed); Mul/Div parity (INV-COMP-3); LSP drift (5704/5707/5922).
- Not checked: GUI display surfaces, reify-mcp/reify-doc renderings.

## Q7 — Systematic detection

Verdict: an audit pattern is the WRONG tool for this class; in-crate tests + a compile-time diagnostic are the right ones. Reasons [verified substrate facts]:
- reify-audit's deterministic substrate is flat line-scanning (no syn/regex/AST); the facts here are linkable Rust consts/functions — a test compares real values where a pattern would scrape text approximations. reify-audit does link reify-compiler (via reify-test-support) but not stdlib/eval/lsp.
- The drift is introduced by code merges → the merge verify gate (every landing) is the right interceptor, not a periodic sweep. Even PTODO's "hard gate" is a verify-pipeline test shelling the binary.
- The strongest detector is the compiler itself: 5371's closed-world manifest turns the whole class into a compile-time diagnostic on every user compile.
- The genuinely uncovered residue: compiler-static-type vs eval-Value-kind EXECUTED parity for names in both registries, beyond 5055's geometry scope and β3's Mul/Div — file as an extension of registry_drift_tests.rs / sibling in reify-eval, driven by the (future) registry table.
- If periodic visibility is wanted later, a thin PREGDRIFT pattern (sketched: eval-only-name / lsp-unbacked-name / lsp-return-mismatch taxonomy, regdrift:allow escapes, committed baseline) becomes nearly free AFTER the manifest exists and is text-scraping guesswork before it. Sequencing, not either/or.

## Proposed next steps (pending Leo's agreement — nothing filed, nothing changed)

1. Agree the two-track shape: interim fallback closure per fail-closed rollout (through rewritten 5371 + 5979 + 5380 rulings, after 5344, Error-severity gated on RU γ/δ) + activate 5068 for the Wave-3 registry PRD with result types in the table.
2. Decide whether to file the NEW finds: (A) fn-return reconciliation [high]; (C) overload-resolution parity pin; (F) port/sub residue; (D)/(E) low.
3. Decide the executed static-vs-runtime parity harness as part of the registry PRD (the "residue" enforcement).
4. No audit pattern now; revisit thin PREGDRIFT post-manifest.
