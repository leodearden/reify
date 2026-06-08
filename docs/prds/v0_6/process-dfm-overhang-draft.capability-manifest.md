# Capability manifest — `process-dfm-overhang-draft.md`

Mechanizes `/prd` gates **G3** (substrate exists) + **G6** (premise valid) per leaf, per
`.claude/skills/prd/project.md` → *Capability Manifest*. Each row binds a leaf's asserted capability
to **evidence** (file:line on main, or a queued producer) + a **verdict**. Any **FAIL** binding
blocks queueing until resolved. Authored 2026-06-08 against the 2026-06-08 feasibility sweep.

Evidence forms used: **wired-on-main** (anti-orphan — symbol reached from a production dispatch
path, not test-only), **grammar-fixture** (G3 — novel syntax parses, or names a producer),
**numeric-floor** (G6 — `bound > floor`, or "no numeric bound asserted"), **field-population**
(producer writes a real non-`Undef` value). Verdicts: **PASS** / **PASS (producer-self)** (the leaf
*is* the producer of a not-yet-wired capability — wiring is the leaf's own scope) / **FAIL**.

---

## Leaf α — overhang + draft eval-level selectors (`reify-eval/src/topology_selectors.rs`)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| Per-face outward normal queryable | wired-on-main | `GeometryQuery::FaceNormal` (`reify-ir/src/geometry.rs:1068`) → OCCT `lib.rs:2972` → `query_face_normal` (`ffi.rs:710`); consumed by `faces_by_normal` (`topology_selectors.rs:600-606`) in the production selector path | PASS |
| Dot-product-threshold-over-faces compute reusable | wired-on-main | `faces_by_normal` (`topology_selectors.rs:583`) `extract_faces`→`filter_by_value`→`normalize3`/`dot3`/`acos`; `validate_angular_tol` guards (`:589`) | PASS |
| Curved-face facet normals | wired-on-main | `tessellate` (`geometry.rs:2400`; OCCT `lib.rs:3158`) → `Mesh.normals` (`geometry.rs:1561`), outward per `occt_wrapper.cpp:4470-4514` | PASS |
| Direction + angle arg resolution | wired-on-main | `resolve_vec3_arg` (`geometry_ops.rs:4038`), `resolve_angle_scalar_arg` (`geometry_ops.rs:4066`) | PASS |
| Overhang dip / draft / undercut measurement | numeric-floor (G6) | **No numeric accuracy bound asserted.** Planar/conical: exact (single exact normal + exact dot). Curved: per-facet sampled, **worst facet reported** (conservative bound, never optimistic). RED tests assert inequalities on planar fixtures (30° wedge dip == 30°; 60° face not flagged at 45°), never an exact float on a curved surface. No floor to mis-calibrate ⇒ esc-3453/3770 class structurally avoided | PASS |
| New selector fns wired (not test-only) | wired-on-main (producer-self) | α adds the fns; γ (the pass) is the production caller. α's own anti-orphan signal = `cargo test -p reify-eval` exercising them + `rg` showing the fns | PASS (producer-self) |

## Leaf β — `process.ri` surface (`Adding.max_overhang_angle`, `DFMRule.subject : Solid`)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| `param max_overhang_angle : Angle` parses + resolves | grammar-fixture | Precedent on main: `Forming.draft_angle : Angle` (`process.ri:76`); `Angle` is a registered `NAMED_DIMENSIONS` dimension. No novel syntax | PASS (`grammar_confirmed=true`) |
| `param subject : Solid` parses + resolves | grammar-fixture | Precedent on main: `Subtracting.tool_access : Solid` (`process.ri:54`); `"Solid" => Type::Geometry` (`type_resolution.rs:563`). No novel syntax | PASS (`grammar_confirmed=true`) |
| `subject : Structure` is NOT used | anti-mismatch | `"Structure"` has no `resolve_type_name` arm (`type_resolution.rs:560-589`); it is the purpose-only wildcard sentinel `WILDCARD_STRUCTURE_KIND` (`expr.rs:551`). β types `subject` as `Solid` → no fiction | PASS |
| Required-member enforcement | wired-on-main | The no-default → required-member convention (`process.ri` `Process{duration;cost}`); omission yields a `required_members` diagnostic (β's signal) | PASS |

## Leaf γ — auto-measurement check-time pass (`reify-eval` `Engine::measure_dfm_rules`)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| Kernel-backed check-time eval pass with no-kernel degradation | wired-on-main (template) | `RepresentationWithin` interception (`engine_constraints.rs:30-90`) + `Engine::check` C1 invariant (`engine_constraints.rs:654-685`): empty realized data → `Indeterminate`, never false `Violated` | PASS |
| Realize a `subject` solid handle at check time | wired-on-main | `tessellate_realizations` populates realized data pre-`check()`; internal handle queries `engine_build.rs:756/798` | PASS |
| Duck-type `DFMRule` instances (read `severity`/`applies_to`/`subject`) | wired-on-main | `dfm.rs::parse_dfm_severity` (`dfm.rs:134-151`) already duck-types a `StructureInstance` on its `severity` field — same read pattern | PASS |
| Read category-specific capability (`Adding.max_overhang_angle` / `Forming.draft_angle`) from `applies_to` | field-population (producer-self) | β supplies the params; γ reads the conformer's concrete-type fields. γ is the production consumer (its signal = `reify check` auto-emits with no hand-declared feature) | PASS (producer-self) |
| New pass invoked from `Engine::check` (not orphan) | wired-on-main (producer-self) | γ adds the invocation in `Engine::check`; anti-orphan signal = `rg` shows `measure_dfm_rules` called from `Engine::check` + the e2e flip in ζ | PASS (producer-self) |

## Leaf δ — `dfm::diagnose` overhang/draft/undercut arms (`reify-stdlib/src/dfm.rs`)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| `DFMSeverity` → `Severity` bridge | wired-on-main | `dfm::diagnose` (`dfm.rs:249`), `parse_dfm_severity` (`dfm.rs:134`), `build_volume_violation` (`dfm.rs:174`) — the success-path severity bridge; δ adds sibling arms | PASS |
| `diagnose` reached from a production path | wired-on-main | re-exported `crate::dfm_diagnose` (`reify-stdlib/src/lib.rs:31`); γ routes results through it (the pass is the new caller) | PASS |
| Overhang/draft/undercut diagnostics emit at the rule's severity | field-population (producer-self) | δ adds `{I,W,E}_DFM_OVERHANG`/`_DRAFT` + `E_DFM_UNDERCUT`; signal = `cargo test -p reify-stdlib` asserting severity mapping (mirrors the shipped `diagnose_violation_*` tests `dfm.rs:569-643`) | PASS (producer-self) |

## Leaf ε — `engine-integration-norm.md` §3 entry

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| The norm admits a PRD-introduced §3 entry in its own commit | wired-on-main | Norm §3 governance (`engine-integration-norm.md:151`) + §13 Q2 option (a) | PASS |
| The new seam is real (not §3.1–§3.7) | anti-mismatch | The check-time DFM-rule walk fits none of §3.1–§3.7 (verified in the feasibility sweep); selectors ride existing §3.1 | PASS |

## Leaf ζ — end-to-end CI example + doc reconcile (user-observable leaf / integration gate)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| `reify check` auto-emits a DFM diagnostic with NO hand-declared measured feature | wired-on-main (integration gate) | Composes α (measure) + β (surface) + γ (pass) + δ (diagnose). The `.ri` declares part + `DFMRule` only; the overhang/draft value is engine-measured. This is the G2 user-observable leaf | PASS (gated on α–δ) |
| Example fixtures are planar (G6-honest) | numeric-floor | Wedge/box fixtures → exact planar measurement; no curved-surface exact-float assertion | PASS |
| Doc-reconcile claims match shipped behavior | anti-mismatch | `docs/reify-stdlib-reference.md` §8 updated to the auto-measurement engine + `subject : Solid`; verified against the green example | PASS (at ζ) |

---

## Summary

No **FAIL** bindings. The producer-self rows (α selectors, γ pass + capability reads, δ diagnostic
arms, γ pass-invocation) are the standard "the leaf is the producer; its own observable signal +
`rg`/`cargo test` is the anti-orphan evidence" pattern, not orphans. **G6:** no numeric accuracy
floor is asserted anywhere — overhang/draft are exact-planar / conservative-sampled-curved angle
comparisons, and all RED tests assert inequalities on planar fixtures. **G3:** no novel grammar
(`Angle`/`Solid` param precedents on main); `subject : Solid` avoids the unregistered `Structure`
type. The batch is clear to queue.
