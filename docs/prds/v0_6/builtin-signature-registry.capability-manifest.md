# Capability manifest — builtin-signature-registry

PRD: `docs/prds/v0_6/builtin-signature-registry.md`. Binds each leaf's asserted capabilities to evidence (G3+G6 mechanized). Machine-readable twin: `builtin-signature-registry.capability-manifest.yaml`. All source anchors verified 2026-08-04 against main `5fb3959fbb` (evidence lineage: the 2026-08-03 measured enumeration, session `investigate-reify-995019`, `xref.json`).

**D3 verification note (2026-08-04, run `wf_53a7db08-66a`)**: the γ workflow's adversary correctly FAILED the α leaf's original probe-based registration premise as VACUOUS — `reify check` exits 0 on an unregistered name (`tests/prd-gate/fixtures/unknown_fn_silent_accept_baseline.ri`), on a bogus match-variant name, and on a mis-annotated payload (controls run 2026-08-04). Resolution: registration/typedness evidence re-homed to source-level wired bindings below; `.ri` fixtures serve as parse/regression baselines only. This vacuity is itself the hole the PRD closes (boundary #8).

| Leaf | Capability | Binding | Verdict |
|---|---|---|---|
| α | parse fns registered+typed today (seed family) | wired — `crates/reify-compiler/src/parse_signatures.rs:22` `PARSE_FN_NAMES`; eval arm `crates/reify-stdlib/src/parse.rs:17`; regression fixture `tests/prd-gate/fixtures/parse_length_match_resolves.ri` (non-probative for registration, see D3 note) | PASS |
| α | analysis fns registered (seed family) | wired — `crates/reify-compiler/src/analysis_signatures.rs:38` `ANALYSIS_FN_NAMES` (5 names) | PASS |
| α | `macro_rules` registry substrate | Rust language substrate; strum workspace-dep precedent for derives (`reify-ir` `GeometryOp` discriminants) | PASS |
| α | row-without-eval-arm build failure | producer: this leaf (negative recipe deliverable, doc-comment form per 5055 precedent) | PASS |
| β | `CompiledExpr` lives in reify-ir | wired — `crates/reify-ir/src/expr.rs` (FunctionCall kind; task 3702 lineage) | PASS |
| β | content-hash currently excludes result_type | wired — `crates/reify-compiler/src/expr.rs:3587-3594` (hash = tag + qualified_name + arg hashes) | PASS |
| τ1 | numeric eval arms exist (floor/ceil/round → Int; log10 → Real) | wired — `crates/reify-stdlib/src/numeric.rs:37-42`; sinh/cosh/tanh `crates/reify-stdlib/src/trig.rs:38-40` | PASS |
| τ1 | dimensioned silent-accept pre-state | fixtures `numeric_floor_dimensioned_silent_accept.ri`, `numeric_sinh_dimensioned_silent_accept.ri` (check exit 0 today, D3-verified); probe evidence `floor(2.5mm)` → `Int(0)` (2026-08-03 sweep) | PASS |
| τ1 | two-arg floor call syntax parses | grammar-fixture — `tests/prd-gate/fixtures/numeric_floor_two_arg_parses.ri` (0 ERROR nodes, D3-verified) | PASS |
| τ1 | two-arg floor eval semantics | producer: this leaf (new capability — `floor(x/q)·q`; not asserted as existing) | PASS |
| τ1 | prose deferral to close | wired — `crates/reify-compiler/src/math_signatures.rs:90-92` (uncited deferral comment) | PASS |
| τ2 | orientation/frame eval arms exist | wired — `crates/reify-stdlib/src/orientation.rs` (`orient_*` arms :10-563), `crates/reify-stdlib/src/geometry.rs:512` (`frame_to_frame`) | PASS |
| τ2 | frame_to_frame mistyping pre-state | fixtures `frame_to_frame_resolves.ri` (check exit 0 silent today), `frame_to_frame_transform3_nomatch.ri` (D3-verified) | PASS |
| τ2 | member syntax on nominal returns parses | grammar-fixture — `orient_axis_angle_member_parses.ri`, `orient_to_axis_angle_call_resolves.ri` (D3-verified) | PASS |
| τ2 | orientation ctor registrations upstream | producer: task 5344 (in-progress) — hard `add_dependency` edge wired at decompose | PASS |
| τ2 | nominal-structure-over-Map precedent | wired — `crates/reify-compiler/src/joint_signatures.rs` `joint_ctor_result_type` (:128) types Map-valued joint builtins as StructureRef | PASS |
| τ3 | joint family eval arms + compiler table | wired — `crates/reify-stdlib/src/joints.rs:193-719` (`transform_at`, `joint_*`); `joint_signatures.rs:69` (17 names) | PASS |
| τ4 | FEA/flexure/stackup eval arms | wired — `crates/reify-stdlib/src/fea.rs:47-102`, `flexures/*.rs`, `stackup.rs:17-21`, `dfm.rs:32`, `tolerancing.rs:18-19`, `supports.rs:116-134`, `loads.rs:134`, `tensegrity.rs:22-23` | PASS |
| τ5 | mechanism/trajectory eval arms | wired — `crates/reify-stdlib/src/mechanism.rs:42`, `snapshot.rs:449-481`, `sweep.rs:141`, `dynamics/eval.rs:160-164`, `trajectory/mod.rs:70-104` | PASS |
| τ5 | piecewise_polynomial is a stub | wired (as stub) — `trajectory/mod.rs:104` always-Undef; row ledgered + PTODO cite required (INV-SF-5) | PASS |
| τ6 | complex family eval arms | wired — `crates/reify-stdlib/src/complex.rs:65-272` (`re`/`im` aliases + `complex_*` ops) | PASS |
| τ7 | compose is `.ri`-owned | wired — `crates/reify-compiler/stdlib/fields.ri:118` typed generic decl (task 4224); resolution fixture `compose_fn_field_resolves.ri` + rejection controls `compose_one_arg_rejected.ri`, `compose_middle_type_mismatch_rejected.ri` (D3-verified) | PASS |
| τ8 | eval-side consumer maps exist | wired — `crates/reify-eval/src/geometry_ops.rs` (`is_geometry_query_call`:3856, selector map :5314-5352); drift test `crates/reify-eval/src/registry_drift_tests.rs` (task 5055) as migration validator | PASS |
| τ9 | GeometryOp descriptor table to bridge | wired — `crates/reify-ir/src/geometry.rs:569` strum discriminants (4670-4675 program) | PASS |
| π | LSP table to replace | wired — `crates/reify-lsp/src/completion.rs:314` `BUILTIN_FUNCTIONS` (~95 entries; 35 contradict compiler per enumeration) | PASS |
| ψ | value-kind check primitive exists | wired — `value_type_kind_matches` `crates/reify-eval/src/lib.rs:307-333`; reify-eval deps reify-compiler + reify-stdlib (Cargo.toml) | PASS |
| ω | fallback site + pre-state | wired — `crates/reify-compiler/src/expr.rs:3568-3584`; baseline fixture `unknown_fn_silent_accept_baseline.ri` (check exit 0 today, verified 2026-08-04); flip gated on task 5997 (hard edge) | PASS |
| λ | boundary rows are deliverables | producer: this leaf (integration gate; §8 table is the signal source) | PASS |
| ρ | exemplar auto-compile gate exists | wired — `crates/reify-compiler/tests/examples_smoke.rs`; `examples/best_practices/INDEX.md` present | PASS |

No binding resolves to a blocking class (`declared-only` / `test-only` / `producer-downstream` / `producer-absent` / `fixture-ERROR` / `bound≤floor` / `rejection-absent`). The one D3 FAIL set was adjudicated by re-homing evidence (see note), not by waiving it.
