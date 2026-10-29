# PRD: GUI backend — engine.rs modules, one session event sink, debug writes on the EvalQueue, one AI tool contract

**Milestone:** v0_6 · **Status:** active — authored 2026-10-13/14; decomposed 2026-10-14 (pass 2): α #8937 … τ #8955, ω #8956 close (ids in §10 and the sidecar); rulings OQ1–OQ5 applied (§7); gap-fill 2026-10-16: μ₂ #9278, λ #8947 / ξ #8950 notes · **Approach:** B + H

Authored 2026-10-13 by the S25 `/prd` session of program `review-all-reify-20261010`
(`plans/review-all-program-reify-20261012.md`, stream **S25 GUI backend**, wave 2, mode spawn,
L/high). Ruling source: the program doc §Resolved decisions → "S25 GUI backend: engine.rs
modules, one event sink, EvalQueue for debug writes, shared tool contract" (Leo, 2026-10-12,
MCP tool clause STRENGTHENED). This PRD does not relitigate that ruling; §2 records where the
code at main differs from the review's premises, and §7 records Leo's 2026-10-13 rulings on the
four questions this session put to him.

**Code anchors** verified against main `b1a7be2982` (2026-10-13). The review's anchors were
verified at `a030ee7528` (2026-10-10); `git diff a030ee7528..main` is empty for
`gui/src-tauri/src/engine.rs` and `gui/src-tauri/src/tests/engine_tests.rs`, and for
`debug_server.rs` shows only the write-confinement work (#7816: `write_confinement.rs`,
`confined_write_target`) and the pre-bound debug listener. Re-checked at `cf59570562`
(2026-10-14): no change on any S25 path. Main moves fast — cite-by-symbol; re-locate lines at
implementation time.

---

## §0 — Goal and user-observable surface

The GUI backend (`gui/src-tauri/src`) is the layer between the evaluation engine and the
Tauri/webview frontend, plus the reify-debug HTTP MCP server the sidecar assistant and the
visual e2e harness drive. Its two largest files hold most of its defects: `engine.rs` (9,587
lines, half of them prose) and `debug_server.rs` (5,634 lines). Around them sit eight bespoke
engine-to-GUI side channels, two scheduling mechanisms for the same engine writes, and two
implementations of the same five AI write tools that disagree on substance.

After this PRD lands, a user observes:

- **Switching the active FEA case keeps the viewport's routing.** Today, on a design with a
  `DisplayOutput` pane and an FEA multi-case result, a case switch publishes an empty
  `display_panes`/`display_appearance` and strips every mesh's material appearance: all meshes
  collapse into pane 0 and style overrides vanish until the next full rebuild
  (`tests/prd-gate/fixtures/s25_case_switch_display_routing.ri`).
- **The assistant's `reify_set_parameter` means one thing.** Its schema, its validation and the
  GUI's write-back agree: a dimensioned cell takes a unit-bearing literal (`120mm`), a bare
  number only a dimensionless cell (OQ1). Today reify-mcp's schema advertises the opposite of
  what the GUI host enforces.
- **AI and debug writes are ordered with the user's own edits.** A debug-driven write is queued
  behind (and coalesced against) GUI evaluations, toggles the evaluation status the frontend and
  `wait_for_idle` read, and replies with the generation it was published under. Today these
  writes run on a fresh 256 MiB thread beside the queue, under a "debug ops never overlap"
  assumption restated 14 times.
- **A frontend refusal is reported as a refusal.** `set_fea_case` no longer returns
  `{"ok": true}` while the frontend rejected the state.
- **Adding a debug tool, an AI tool, a GuiState field's sync or a session event reds a named
  test or fails to compile** instead of passing through a hand-kept list, a regex over Rust
  source, or an emitter nobody installed.

The stream's high-severity goals are the engine.rs partition (fk-05311be442e6) and the one AI
tool contract (fk-bb84d4272cfd). Both are behaviour-preserving restructurings whose payoff is
the next change: today an agent adding a unit spelling, an FEA channel or an AI write tool has
to find a 300–700-line group inside a 9.6k-line file, or learn that three implementations of a
tool exist and already disagree.

## §1 — Background: findings ledger

Every key the stream owns, with the leaf that closes it (§10). One key, one leaf; a leaf named
"contributing" moves part of a key's measurement without closing it.

| key | anchor | sev | verdict | leaf |
|---|---|---|---|---|
| fk-05311be442e6 | `engine.rs` — one file, ~10 independent pure-function groups, tests reach them crate-privately | high | confirmed | ε (α β γ δ contributing) |
| fk-b28eb5276628 | `engine.rs::set_active_fea_case` — four GuiState literals, case switch empties panes/appearance | medium | confirmed | η |
| fk-4ecb76174487 | `engine.rs::EngineSession` — eight side channels, telemetry routing enforced per entry point | medium | confirmed | λ |
| fk-2dcacd59c79d | `engine.rs::EngineSession` — reload pipeline open-coded four times; tuple orders reversed | medium | confirmed | ζ |
| fk-995c30a8a446 | `engine.rs::surface_geometry_derived_cells` — logic reads wire-DTO strings back | medium | confirmed | θ |
| fk-55213deb2362 | `engine.rs` — string-keyed Value decodes; finish mapping twice | low | confirmed | ι (κ contributing) |
| fk-e2b409ec7580 | `debug_server.rs::dispatch_tool` — tool identity as strings across Rust/TS lists | medium | weakened | τ |
| fk-58f6f0d58857 | `tests/debug_write_tool_routing_tests.rs` — INV-GUI-2 held by a Rust-source lexer | medium | confirmed | ο |
| fk-c4da71a0014e | `debug_server.rs::handle_set_fea_case` — refusal swallowed; baseline advanced first | medium | confirmed | ρ |
| fk-7212abff873d | `debug_server.rs::run_on_engine` — two scheduling mechanisms for the same writes | medium | confirmed | ρ (π contributing) |
| fk-bb84d4272cfd | `reify-mcp tools/write.rs::register` — five AI write tools implemented twice, diverged | high | confirmed | ο (ν ξ contributing) |
| fk-cc4329a75e3e | `reify-mcp types.rs::ParameterInfo` — enum facts as free Strings; four ValueCellKind renderers | medium | confirmed | σ (θ contributing) |
| fk-08fd82603d2b | `engine.rs` (h11, prior verdict, still present) — parallel sync channels: bespoke emitters beside GuiState fields | medium | still_present | μ |
| fk-484c81d71180 | `debug_server.rs` (h7, prior verdict, still present) — debug mutations bypass SnapshotPublisher | medium | still_present | ρ |

The last two rows are the brief's "unmapped canonicals" `gui|…/engine.rs|h11` (p3) and
`gui|…/debug_server.rs|h7` (p4): prior-run verdicts the review re-checked as still present
(record `prior_verdicts`); they carry their original keys.

Weakened finding re-read under the freshness clause: fk-e2b409ec7580's skeptic note refutes
"system-prompt.ts has no parity check" — `gui/src/__tests__/sidecarPromptParity.test.ts`
(task 7049) checks it. The leaf is scoped to what survives: the regex-over-Rust parsers
(`debugParity.test.ts`'s dispatch-arm regex, `toolDefNames.ts`, the INV-GUI-2 lexer) and the
hand-kept name lists. Its Rust dispatch-arm count is 21 literal arms at main (R3), not the
skeptic's 25.

## §2 — Premise corrections found at authoring (G6)

### §2.1 The CLI MCP host is ruled for deletion (#6665)

The ruling says "CLI and Tauri hosts implement the trait only". Task **#6665**
(Leo 2026-08-26, reconfirmed by S27's OQ1-A on 2026-10-12) deletes `reify mcp-server` and
`crates/reify-cli/src/mcp_context.rs` (`CliToolContext`) and keeps the reify-mcp crate and the
GUI's `TauriToolContext`. After it, `TauriToolContext` is the one `ReifyToolContext` host. The
ruling's substance — one implementation per tool, written over the host trait — is unchanged;
the reify-mcp contract leaves depend on #6665 so only one host is edited, and nothing here
touches `crates/reify-cli`. The CLI host's divergent vocabularies (`"Auto(free)"`, status
`"unknown"`, phase `"ready"`) disappear with it (fk-cc4329a75e3e's CLI half closes by deletion).

### §2.2 The engine.rs partition is all-downward, but `_for_test` helpers do not move

The review's 11-unit partition holds at main: every cross-unit edge points downward, no pure
unit reads `EngineSession`, `CoreState`, an emitter or `self`, and there is no cycle (R1a).
Four refinements:

- **Unit 7 is two concerns.** The node-id key parsers (`parse_cell_id`,
  `parse_realization_key`, `parse_constraint_key`) are not unit parsing → `engine/keys.rs`.
- **`scalar_to_f64` is the only tensegrity→mechanism edge.** S15 θ (#8698) deletes it in favour
  of `reify_ir::Value::as_f64`, but #8698 sits behind a seven-task S15 chain (#6941, #8674, #8697,
  #6922, #8686, …). δ does not wait: it moves `scalar_to_f64` verbatim into `engine/mechanism.rs`
  (`pub(super)`; tensegrity imports it from there — a sibling edge, no cycle), and #8698's
  delivered check is widened to the new home (§8, OQ5; lead cross-reference, since #8698 is S15's).
- **The 30 `#[cfg(test)]` helpers are all inherent `EngineSession` methods** reaching private
  fields (`parsed_cache`, `line_offsets_cache`, `consumed_idents_cache`, `compile_failure`,
  `core`). The pure units need none of them; the finding's "28 `_for_test` helpers deletable"
  overstates what a partition alone removes. What the partition turns into stated interfaces is
  the ~36 distinct `crate::engine::<free fn>` test imports. The emitter-driver helpers
  (`check_and_emit_for_test`, `emit_*_for_test*`, `drain_and_emit_warm_pool_events_for_test`)
  go with λ; `load_from_compiled`'s hand-copied resets go with ζ.
- **engine_tests.rs's "unit 7" is a remainder, not a unit** (lines 7180–22615, 264 of 458
  tests). Its sub-clusters (R1a Q5) distribute to the module test files by the engine unit
  under test; whatever is genuinely session-core stays in `engine_tests.rs`.

### §2.3 The case-switch defect is wider than recorded

`set_active_fea_case` also builds `meshes` from `tess_mesh_cache`, whose `MeshData` carry
`appearance: None`, so a case switch strips every mesh's resolved material appearance, not
only the pane and appearance directives. The empty lists are published as real deltas
(`display-panes-update` with `[]`), and `engineStore.setDisplayPanes` replaces the whole list
(R1b §A). No test pins panes, appearance or mesh appearance across a case switch.

### §2.4 Telemetry covers six of eight channels; `fea_convergence` does not fit `diffed whole`

`post_engine_call_telemetry` fires AutoResolve, FeaCase, ModeShapeFrame, FeaDiagnostics,
FeaConvergence and the WarmPool drain; SolveCancellation rides `with_solve_slot` and
SolverProgress is emitted inside the kernel through `reify_eval::SolverProgressSink` (R2). The
`gui_state!` macro's `diffed whole` arm accepts only `Vec<T>`; `fea_convergence` is
`Option<FeaConvergenceInfo>` and needs a new arm that distinguishes "unchanged" from "cleared".
Today the debug server's write funnels discard the delta they compute but push the whole state
(`apply_gui_state` → `initFromState` → `snapshotFields`, which seeds `fea_diagnostics` and
`fea_convergence`), so debug writes reach the FEA fields without the emitters. The one debug
write whose FEA fields reach the frontend only through that push is `set_fea_case`
(`set_active_fea_case` fires no emitter); ρ converts it to delta-only, so ρ depends on μ (which
puts the FEA fields on deltas), not the reverse (critic C1-03). The other debug writes ο converts
still commit through telemetry, so their emitters keep firing until μ.

### §2.5 The stdlib-root literal is #5519's, not S25's

The `"crates/reify-compiler/stdlib"` literal sits inside `compile_entry_with_imports`, which
resolution-unification ε (#5519) deletes wholesale. S27's #8769 does not edit the GUI. S25 does
not touch the literal; a cross-reference asks #5519 to use `dev_stdlib_root` (#8769) if its
`compile_program` wiring still needs a dev fallback.

### §2.6 Overlapping filed work

- **#7854** is the debug-server→EvalQueue task; subsumed (its `wait_for_idle`
  note is stale — #6752 landed `7e0d366f3e` and the inline `with_engine_lock` is gone).
- **#6964** (pending, medium; #6338 done, so dispatchable) lands debug-server-gating-boundary α
  (`docs/prds/debug-server-gating-boundary.md`, Leo-ruled D3/D5 2026-08-19 and esc-7240-11
  2026-10-05): the non-Tauri half of `debug_server.rs`, `tool_defs` and all its tests move to an
  ungated `debug_protocol.rs`. S25 builds on that cut (§6 D12); every S25 leaf that edits
  `debug_server.rs` depends on #6964.
- **#8595** (value-input cluster out of engine.rs) and **#8600** (FEA channel block out of
  engine.rs) are subsumed by α and β. **#7471** (ConstraintStatus enum, retire the
  `constraintVerdictTokens` scraper) by θ for its Rust half and S26 for its TS union. **#7722**
  (ToolRegistry engine-bearing classification) and **#7071** (host-side diagnostics filter) by ν;
  **#7181** (host save_file confinement and interlock) by ξ.
- **Ordered, not subsumed:** #8680 (S3 κ: `sync_observed_demand` → `set_observed_demand`; it
  declares itself the only engine.rs edit before S25's split) precedes α. **#6843** (typed verdict
  union; depends on #6898's vocabulary widening) follows θ — its premise stands. #7716
  (open_file opt-in return-on-enqueue + health starvation) and #8574 follow ρ; #8074, #5101 and
  #7615 follow ο (they name the deleted scanner or the save/export rebuild). #5519 races ζ by
  ruling OQ4 (no edge).
- **S6 has filed** (`docs/prds/v0_6/kinematics-typed-kinds-and-mechanism-record.md`): S6 ρ #8845
  builds `MechanismTree::from_mechanism(&Value)`, the one decoder of the mechanism Value, and
  λ₀ #8838 / λ #8839 already edit the GUI mechanism unit (`extract_joint_descriptor` via
  `JointKind`; the raw `bind` form removed, which dissolves `resolve_driving_params_from_ast`'s
  duplicated BindValue scan). κ consumes them rather than being held.
- **S8 ι #8907** edits the three `Value::Field` destructures in `engine.rs`; serialised against
  the partition by file lock (program doc S8 cross-reference), no edge.

### §2.7 The debug server at main

R3 reproduces the review's "13 engine closures = 6 writes + 7 reads" counted per handler (six
`*_and_refresh_baseline(` funnel calls, seven read-only `run_on_engine(` sites); counted as raw
`run_on_engine(` call sites it is 10 (one added since the review by `confined_write_target`).
`handle_set_fea_case` drops the push reply and returns `{"ok": true, "case": <name>}`.
`compute_delta` is called at three non-test sites, always for its side effect, and nothing reads
`DebugServerState.last_state`. One of the 14 serial-debug-ops restatements is code, not prose: a
`#[cfg(debug_assertions)]` `debug_assert_eq!` canary in `open_path_into_engine`. Two single-file
readers break on any split: the INV-GUI-2 scanner and `gui/src/__tests__/toolDefNames.ts` read
only `debug_server.rs` (#6964 makes them read both files; ο and τ retire them). Tool counts: 69
`ToolDef`s, 21 with literal Rust arms, 48 falling through to `bridge.ts`.

### §2.8 Behaviour differences between the two AI write-tool implementations

Beyond the value grammar, the debug server's five `reify_*` tools carry behaviour the reify-mcp
path lacks: write confinement on save-as/export (#7816's `write_confinement.rs`), the
update_source active-file guard, the rejected-buffer save interlock, path-matched diagnostics
filtering, and the frontend push. reify-mcp's `reify_update_source` hard-codes `success: true`.
`reify_write_tool_envelopes_match_the_reify_mcp_shapes` compares hand-copied key sets and never
touches reify-mcp. Delegation must therefore move each behaviour to a home rather than drop it —
§4.10's placement table.

## §3 — Sketch of approach

Five strands, each landing behaviour-preserving steps first.

1. **Partition** (α–ε, move-only). `engine.rs` stays the module root (so every pending task's
   `gui/src-tauri/src/engine.rs` path still names the session core) and gains child modules
   under `gui/src-tauri/src/engine/`. Each child's `pub(crate)` items are its stated interface;
   tests move out of `engine_tests.rs` into `tests/engine_<module>_tests.rs`. After ε,
   `engine.rs` holds `EngineSession`, `core_state`, `CompileFailure*`, the session-event seam and
   the compile helpers; nothing else.
2. **Session internals** (ζ η θ ι κ). One `reload()` owns check → commit → telemetry → build;
   one post-tessellation builder assembles every non-preview, non-cold-start GuiState; five
   closed-set wire fields become serde enums (wire bytes unchanged) so engine logic compares
   typed values; eval Values the GUI reads are decoded once per shape, the mechanism through S6's
   `MechanismTree`.
3. **One session event sink** (λ μ). The eight trait/field/setter/impl/double sextets become
   one `SessionEvents` trait installed once; a commit hands back a `#[must_use]` receipt that
   only `post_engine_call_telemetry` consumes (a compile error under the crate's
   `deny(unused_must_use)`); `fea_diagnostics`/`fea_convergence` move onto deltas before debug
   writes stop pushing whole states.
4. **One AI tool contract** (ν ξ ο σ, §4.10–§4.11). reify-mcp owns one typed request/response
   pair, schema and engine class per tool (ν); the GUI host implements the trait and owns every
   host behaviour (ξ); the debug server delegates the five `reify_*` names through one queued job
   constructor, replies stamped with their generation, and the dead `mcp_tool_call` command and
   the INV-GUI-2 source lexer go (ο); wire enums replace free strings (σ).
5. **Debug writes on the EvalQueue** (π ρ τ, §4.7–§4.9, §4.12). The debug server reaches the
   engine only through a `QueuedEngine` capability whose jobs run on the ENGINE lane and whose
   only publisher is `SnapshotPublisher` (π); every remaining debug engine access moves onto it
   and each tool's publication follows OQ2 (ρ); the tool set becomes a typed table exported as
   JSON (τ).

Consumers (G1): the GUI frontend (viewport, property panel, editor) through the published
state; the sidecar assistant through `mcp__reify-debug__*`; the visual e2e harness through
`docs/debug-mcp-contract.md`; S26 through `types.rs` enums, the `SessionEvents` payload set and
the exported debug tool table.

## §4 — Contract (H)

### §4.1 engine.rs module map (α–ε)

Units, homes and edges (R1a; symbol lists are the review record's h14 entry for
`gui/src-tauri/src/engine.rs`, corrected per §2.2):

| child module | holds | imports (downward only) | tests move to |
|---|---|---|---|
| `engine/units.rs` | unit/quantity parsing: `normalize_unit_label`, `superscript_label_spelling`, `ComposedUnit`, `ladder_dimension`, `LADDER_COVERAGE`, `dimension_requires_unit`, `COMPOSED_UNIT_INDEX`, `composed_unit_index`, `resolve_quantity_suffix`, `parse_value_string`, `unwrap_optional`, `declared_scalar_dimension`, `parse_value_string_for_cell`, `unit_hint_from_default_literal`, `unit_is_emittable_as_written`, `describe_unit_expr`, `expr_kind_name` | reify_core, reify_ast, `crate::display_units` | `tests/engine_units_tests.rs` |
| `engine/keys.rs` | `parse_cell_id`, `parse_realization_key`, `parse_constraint_key` | reify_ir | with units |
| `engine/line_col.rs` | `offset_to_line_col_fast` (the table-driven sibling `reify_core` lacks); the `pub(crate) use reify_core::{…}` shim is deleted and its 17 test references repoint to `reify_core::build_line_offsets` | reify_core | `tests/engine_line_col_tests.rs` |
| `engine/write_atomic.rs` | `write_file_atomically` | std | — |
| `engine/persistent_cache.rs` | `sweep_persistent_cache`, `bootstrap_persistent_cache_sweep` (`pub`; main.rs) | std, reify_eval | with line_col's file or its own |
| `engine/fea_channels.rs` | `sample_stride_field_nearest` … `apply_shell_channels` (15 items) | `crate::types`, reify_ir, reify_eval, reify_stdlib | `tests/engine_fea_channels_tests.rs` |
| `engine/display_routing.rs` | `MergedTraitDefs` + its impl, `merged_trait_defs`, `collect_display_routing`, `extract_display_style_data`, `project_appearance`, and the inline `display_style_extract_tests` | `crate::types`, reify_eval | `tests/engine_display_routing_tests.rs` |
| `engine/entity_tree.rs` | `collect_consumed_sibling_names`, `is_geometry_valued`, `build_template_node` | display_routing, projection | `tests/engine_entity_tree_tests.rs` |
| `engine/mechanism.rs` | `extract_joints_from_mechanism` … `collect_consumed_mechanism_idents` (with `scalar_to_f64`, `pub(super)`: tensegrity imports it from here; #8698 later deletes it) | reify_ast, reify_ir | `tests/engine_mechanism_tests.rs` |
| `engine/tensegrity.rs` | `build_tensegrity_wires` … `surface_data_from_instance` | `crate::types`, reify_ir | `tests/engine_tensegrity_tests.rs` |
| `engine/projection.rs` | Value→DTO projection: `cell_kind_gui_str`, `cell_kind_tree_str`, `display_scalar`, `value_as_declared`, `format_determined_cell`, `declared_dimension_index`, `build_values`, `satisfaction_token`, `build_constraints`, `surface_geometry_derived_cells`, `recheck_indeterminate_constraints`, `build_preview_gui_state`, `diagnostics_to_info`, and the expression printer (`is_identifier_like`, `format_expr`, `collect_value_refs`, inline `format_expr_tests`) | units, keys, line_col | `tests/engine_projection_tests.rs` |
| `engine.rs` (root) | `EngineSession` + both impl blocks, `mod core_state`, `CompileFailure*`, the session-event seam, `SolveFinishedGuard`, `module_key`, `source_key_matches_path`, `is_stdlib_path`, `build_err_payload`, `parse_errs_to_payload`, the compile helpers, `entry_declares_entity` | every child | `engine_tests.rs` (session core only) |

`impl EngineSession` (3,879 lines, half of them doc prose) stays whole in `engine.rs`: the record
keeps it whole, and a further cut would need sibling `impl EngineSession` blocks writing private
fields — the satellite heuristic 13 forbids. ε records the root's residual size.

Rules every partition leaf follows: move only (no rename, no signature change beyond the
`pub(crate)` widening R1a lists for ~21 cross-unit items); no re-export shim at the old path
(heuristic 13 — callers and tests repoint); tracing `target:` literals stay verbatim; the
`#[cfg(test)] impl EngineSession` block stays in `engine.rs` (moving it would make a satellite
writing the parent's private fields — the record's own symptom).

### §4.2 One reload pipeline (ζ)

```rust
/// What a compile produced, in one order everywhere.
pub(crate) struct CompiledSource { pub parsed: ParsedModule, pub compiled: CompiledModule }

fn compile_single_file_with_stdlib(content: &str, module: &str)
    -> Result<CompiledSource, CompileFailure>;
fn compile_entry_with_imports(entry: &Path, source: &str, module: &str)
    -> Result<CompiledSource, CompileFailure>;

impl EngineSession {
    /// The one reload pipeline: record_compile_failure on Err; else
    /// check_with_solve_slot → commit_state → post_engine_call_telemetry → build_gui_state.
    fn reload(&mut self, compiled: Result<CompiledSource, CompileFailure>,
              source: &str, file: FilePathUpdate) -> Result<GuiState, String>;
}
```

- `load_from_source`, `load_file` and `update_source` choose a compile helper and call
  `reload`; none calls `check_with_solve_slot`, `commit_state` or
  `post_engine_call_telemetry` itself. `update_source` no longer swaps a tuple.
- The copy-pasted error-severity filter becomes one helper both compile functions call.
- `load_from_compiled` (test-only) runs `reload`'s post-compile half; its hand-copied cache
  resets are deleted, which restores the `geometry_derived_cache.clear()` it lost.
- Ordering vs resolution-unification ε (#5519), per OQ4: ζ does not wait. #5519 later replaces
  the producer behind `reload`'s callers at one site; because `compile_program` returns a bare
  `CompiledProgram` (no `Result`, no `ParsedModule`), #5519 derives `CompileFailure` from the
  entry's Error diagnostics and keeps the GUI's own parse for `parsed_cache` — a cross-reference
  line tells its implementer.

### §4.3 One post-tessellation GuiState builder (η)

- The tessellation cache keeps what a rebuild needs: the bare meshes, each mesh's resolved
  material appearance, and the `collect_display_routing` result (panes, appearance
  directives). `set_active_fea_case` re-derives only the FEA channels and FEA fields from it.
- One builder, `EngineSession::assemble_gui_state`, assembles a GuiState from (cached
  tessellation, values/constraints projection, FEA overlay for the active case). `build_gui_state`'s success return and
  `set_active_fea_case` both call it; neither lists fields. The cold-start return and
  `build_preview_gui_state` (a separate throwaway engine) keep their own literals — they are
  "no data here" states by design.
- The case-switch test, RED first: load `tests/prd-gate/fixtures/s25_case_switch_display_routing.ri`,
  build, switch to `overload`; the switched state's `display_panes`, `display_appearance` and
  every mesh's `appearance` equal the built state's, and the stress channels differ. Reading
  that fixture from a Rust test registers it in the rust-coupled fixture list in the same diff
  (`_RUST_COUPLED_RI_FIXTURES` in `scripts/verify.sh` today; `tests/infra/test_verify_scope.sh`'s
  PG-DRIFT enforces it).
- Ratchet row (INV candidate 5, `tests/infra/adoption-ratchet.manifest`): `gui-state-literal`,
  counting `GuiState {` struct-literal lines in `gui/src-tauri/src/engine.rs` at its
  post-extraction count (cold start + builder), owner η.

### §4.4 Closed-set wire fields as serde enums; logic over typed values (θ)

Per OQ3, θ turns the closed-set `types.rs` fields its findings read (four, plus `EntityTreeNode.freshness`, which shares `format_freshness`) into enums whose serde renames keep today's wire bytes:

| field | enum (types.rs) | wire tokens (unchanged) | `From` source |
|---|---|---|---|
| `ValueData.determinacy` | `Determinacy` | `"determined"`, `"undetermined"`, `"provisional"`, `"auto"` (as `format_determinacy` emits today) | `reify_ir::DeterminacyState` |
| `ValueData.freshness` | `FreshnessToken` | `"final"`, `"intermediate"`, `"pending"`, `"failed"` | `&reify_ir::Freshness` (payload collapsed, as `format_freshness` does) |
| `ValueData.kind` | `ValueKind` | `"Param"`, `"Let"`, `"Auto"` | `&reify_compiler::ValueCellKind` (free-ness dropped, as `cell_kind_gui_str` does today) |
| `ConstraintData.status` | `ConstraintStatus` | `"satisfied"`, `"violated"`, `"indeterminate"` | `reify_ir::Satisfaction` |
| `EntityTreeNode.freshness` | `FreshnessToken` | as `ValueData.freshness` | `&reify_ir::Freshness` |

- `format_determinacy`, `format_freshness`, `satisfaction_token` and `cell_kind_gui_str` are
  deleted (`ValueKind`/`Determinacy`/`FreshnessToken`/`ConstraintStatus` implement `From` the
  compiler/IR enums, so the conversion is a type conversion, not a string); every comparison in
  projection (`freshness == "pending"`, `determinacy == "determined"`,
  `status == satisfaction_token(..)` ×2) compares enums.
- Each enum spells its tokens with **per-variant** `#[serde(rename = "…")]`, not `rename_all`:
  the wire tokens stay greppable, which is what #6723's delivered check (widened at decompose to
  `types.rs`, OQ5) resolves against.
- The three non-engine readers (`mcp_context.rs`'s `ParameterInfo.kind`/`.determinacy` and
  `ConstraintInfo.status` fills, which copy the DTO Strings today) render the enum's wire token
  through one `as_wire_str()` per enum until σ replaces them with an exhaustive match onto
  reify-mcp's enums. Whichever of θ and ξ lands second rebases `mcp_context.rs` (no edge: three
  lines).
- `surface_geometry_derived_cells` keys its overlay on the typed `ValueCellId` carried beside
  each projected cell rather than rebuilding one from `entity_path`/`name` strings;
  `recheck_indeterminate_constraints` matches `ConstraintNodeId`s, not `write!`-formatted text.
- From #7471, the Rust half: `gui/src/__tests__/constraintVerdictTokens.ts`, its `.test.ts` and
  the source-reading half of `constraintVerdictParity.test.ts` are deleted — they regex-read
  `satisfaction_token`'s arms, which θ deletes, and their header says they exist only until serde
  enforces the casing. θ does not edit `gui/src/types.ts`: the TS unions are S26's to generate
  (OQ3), and #7471's TS half goes to S26 when #7471 is cancelled.
- `EntityTreeNode.kind` and `cell_kind_tree_str` stay: the tree vocabulary also names non-cell
  entity kinds and feeds `get_entity_identity_map`'s `structural_fingerprint`, whose bytes must
  not move; S26 may type it when it generates the tree type.
- A golden wire test pins the five fields' JSON for one ValueData/ConstraintData per variant,
  so a rename that changes bytes reds.
- Ratchet rows `gui-dto-status-string-compare-root` / `-modules` (Appendix A), ceiling 0.
- #6898 (gui-on-demand-measurement, pending) may widen the verdict vocabulary; after θ it adds
  `ConstraintStatus` variants instead of strings, and #6843 (the TS-side narrowing it waits on)
  follows θ (edge added at decompose).

### §4.5 Decode each eval Value shape once (ι, κ)

- One typed view per result shape the GUI reads, each decoding its `Value` once, field names
  held in one place per shape: `BucklingResult`, `TensegrityWire`, `TensegritySurface`,
  `ElasticResult` (the fields `fea_channels` reads), and the `DisplayStyle`/`Appearance` style
  record. Home: `gui/src-tauri/src/engine/result_views.rs`, imported by fea_channels,
  tensegrity, display_routing and the buckling reader.
- The `"Matte" => 0, "Satin" => 1, "Gloss" => 2` mapping has one home in `display_routing.rs`,
  used by both `extract_display_style_data` and `project_appearance`.
- After ι, outside `result_views.rs`: 0 `type_name == "` tests (result_views compares against
  one named const per type) and no string-keyed field read of a result shape; the residue ι's
  `gui-value-field-get` row records is named in its task record (non-result JSON maps, if any).
  The mechanism map's `Value::String("…")` keys are κ's, under their own row. Rows
  `gui-value-type-name-test`, `gui-value-field-get`, `gui-finish-mapping-copy` (Appendix A).
- ElasticResult: S15 ε (#8690) builds `elastic_result_from_value`, `pub(crate)` and
  persistence-shaped (it copies slab data). ι's view reads the sampled-field references the
  viewport needs and records a breadcrumb (trigger: #8690 exposes a borrowing decoder) for
  folding into it — one home for ElasticResult's field names is S15's to own.
- **κ:** `get_mechanism_descriptors` and `engine/mechanism.rs` read the mechanism Value through
  S6's `MechanismTree::from_mechanism` (S6 ρ #8845) and `JointKind` (S6 β #8829) instead of the
  `Value::Map` keyed `"kind"`, `"bodies"`, `"at"`, `"world"`, `"axis"`, `"range"` (8 lines today,
  row `gui-mechanism-map-key` → 0). S6 λ₀ #8838 already moves `extract_joint_descriptor` onto
  `JointKind`, and S6 λ #8839's removal of the raw `bind` form dissolves
  `resolve_driving_params_from_ast`'s duplicated BindValue scan; κ depends on both and verifies.
  If `MechanismTree` lacks an accessor the descriptor needs (per-joint axis, range), κ adds a
  read-only accessor beside it after #8845 has landed (S6 owns `MechanismTree`; the addition is a
  consumer's read view, named in κ's task record).

### §4.6 One session event sink and a telemetry receipt (λ μ)

```rust
/// Every engine-to-GUI side event, one method each. Installed once.
pub trait SessionEvents: Send + Sync {
    fn auto_resolve_started(&self);
    fn auto_resolve_iteration(&self, it: AutoResolveIteration);
    fn auto_resolve_completed(&self);
    fn warm_pool(&self, ev: WarmPoolEvent);
    fn fea_case_changed(&self, ev: FeaCaseChanged);
    fn mode_shape_frame(&self, frame: ModeShapeFrame);
    fn solve_started(&self, cancel: CancellationHandle);
    fn solve_finished(&self);
    fn solver_progress(&self, p: SolverProgress);
    // fea_diagnostics / fea_convergence: present after λ, removed by μ.
}
```

- `EngineSession` holds one `Arc<dyn SessionEvents>` (a no-op default), set once by main.rs.
  One production impl (`TauriSessionEvents`, main.rs) emits today's event names with today's
  payloads, so no frontend listener changes in λ; `PendingSolveCancelSink`'s slot behaviour
  moves into its `solve_started`/`solve_finished`. The `reify_eval::SolverProgressSink` the
  kernel calls is an adapter over `SessionEvents::solver_progress` installed in the same place.
  One test double, `RecordingSessionEvents`, in `tests/test_helpers.rs`.
- Deleted: the seven engine.rs traits, eight `Option<Arc<dyn …>>` fields and setters, seven
  Tauri impls and the eight per-install lines, the eight Recording doubles, and the
  emitter-driver `_for_test` helpers the double makes unnecessary.
- **Receipt.** `CoreState::commit_state` and `commit_check` return a `#[must_use]`
  `CommitReceipt` with a private constructor; `post_engine_call_telemetry(receipt)` is its only
  consumer. λ adds `#![deny(unused_must_use)]` to `gui/src-tauri/src/lib.rs` (absent today), so a
  new entry point that commits and skips telemetry is a compile error in every build of the
  library, not only under the gate's clippy `-D warnings`; λ fixes whatever existing ignored
  `#[must_use]` values the attribute surfaces, including gui-feature-gated code. The one
  deliberate non-consumer, `inject_check_for_test`, discards its receipt with an explicit
  `let _ =`. This retires the "silently lost — nothing fails CI" paragraph; the
  per-entry-point `*_emits_fea_diagnostics` tests stay as behaviour tests on
  `RecordingSessionEvents`.
- Non-committing mutators (`export`, `sync_observed_demand`, `sync_demand`, `build_gui_state*`)
  take no receipt by design; λ's doc on `SessionEvents` says so, and that warm-pool events from
  their kernel work are drained at the next commit.
- **μ.** `fea_diagnostics` becomes `diffed whole` (event name `fea-diagnostics-changed` kept,
  payload unchanged). `fea_convergence` gets a new `gui_state!` arm for `Option<T>`
  (`diffed whole_opt`: delta field distinguishes unchanged from cleared; `StateDelta::full`
  handles it; event `fea-convergence-changed` with today's `FeaConvergenceInfo | null`
  payload). Both emitter methods leave `SessionEvents`. Behaviour change, stated: the events now
  fire on change, not on every commit — `initFromState`/`applyPublishedState` already seed both
  from the snapshot. μ follows λ and η (the case switch publishes through the builder); ρ follows
  μ, so the debug `set_fea_case` becomes delta-only only after the FEA fields ride deltas (§2.4).
- Docs: `docs/gui-event-channels.md` inventory rows for the eight channels are updated by λ and
  μ (FeaDiagnostics and FeaConvergence rows move to the delta table in μ).

### §4.7 Engine access capability: `QueuedEngine` (π) and stamped replies (ο)

The synthesis's "WriteSeam", realised as the queue itself rather than a token minted beside it.

```rust
// gui/src-tauri/src/eval_queue.rs — added by ο
/// A reply the queue stamped with the generation it ran under.
pub struct Stamped<T> { pub generation: u64, pub value: T }
// EvalRequest gains a stamped evaluation kind: QueuedJob::run receives the entry's generation,
// so a StampedJob<T> replies Stamped { generation, value }.

// gui/src-tauri/src/queued_engine.rs — new top-level module, added by π
/// An engine reachable only through its queue: the debug server's whole engine access.
/// A job borrows the mutex for its own duration on the ENGINE lane and can neither keep
/// it nor reach the delta baseline.
#[derive(Clone)]
pub struct QueuedEngine { engine: Arc<Mutex<EngineSession>>, queue: Arc<EvalQueue> }
impl QueuedEngine {
    pub fn new(engine: Arc<Mutex<EngineSession>>, queue: Arc<EvalQueue>) -> Self;
    pub fn activity(&self) -> EngineActivity;                        // lock-free (engine_activity::probe)
    pub async fn wait_until_settled(&self, r: &SettleRequest) -> WaitOutcome; // lock-free polling
    pub fn call<T: Send + 'static>(&self,                            // engine_call: ordered, no publish
        job: impl FnOnce(&Mutex<EngineSession>) -> Result<T, String> + Send + 'static) -> EvalTicket<T>;
    pub fn evaluate<T: Send + 'static>(&self,                        // stamped evaluation: publishes outcome.publish
        job: impl FnOnce(&Mutex<EngineSession>) -> EvalOutcome<T> + Send + 'static) -> EvalTicket<Stamped<T>>;
    pub fn snapshot(&self,                                           // snapshot: publishes the built state
        job: impl FnOnce(&Mutex<EngineSession>) -> Result<GuiState, String> + Send + 'static) -> EvalTicket<PublishedState>;
}
```

- `QueuedEngine` lives in its own top-level module because it uses both `eval_queue` and
  `engine_activity`, and `engine_activity` already imports `eval_queue`; putting it inside
  `eval_queue.rs` would create the SCC {eval_queue, engine_activity}, which INV-ARCH-1
  (`scripts/assert-module-scc.py check`, #8673) reds. `queued_engine` imports `engine`,
  `eval_queue` and `engine_activity`; `debug_server`, `main` and `mcp_context` import it.
- After ρ, `DebugServerState` is `{ engine: QueuedEngine, selection, frontend:
  Arc<dyn FrontendPort>, registry: Arc<reify_mcp::ToolRegistry> }`. It holds no
  `Arc<Mutex<EngineSession>>`, no `last_state` and no `Arc<EvalQueue>`: the only roads to the
  engine are the three ordered job kinds, and the only road to its liveness is the lock-free
  probe `health`, `engine_status` and `wait_for_idle` already use (heuristics *minimum data
  access scopes*, *deep modules with narrow interfaces*, *clear invariants uniformly enforced*).
- Jobs borrow, they do not capture: every `commands::*_impl(&Mutex<EngineSession>, …)` is reused
  unchanged inside a job; `TauriToolContext<'e>` borrows the mutex and is built inside the job
  (§4.10).
- `SettleRequest` gains `until_generation: Option<u64>` (inclusive), beside `since_generation`;
  `wait_for_idle` reads it from params (π).
- `FrontendPort` (ungated part of `gui/src-tauri/src/debug.rs`, π) is the debug server's only
  road to the webview: `fn query(&self, command, params, timeout) -> BoxFuture<Result<Value,
  String>>`, implemented by `DebugBridge` under the `gui` feature and by a `ScriptedFrontend`
  test double — which makes the handlers drivable headlessly with the existing `ManualQueue` /
  `RecordingObserver` helpers.
- **Residual bypasses, stated.** `AppState.engine` stays `pub` for Tauri commands, the watcher
  and setup, and `commands::*_impl` stay `pub`: new GUI-side code can still lock the engine
  off-queue. The guarantee is per module (the debug server and the AI tools), not crate-wide.
  A job cannot `await`, so a nested submit can only deadlock through an explicit `block_on`
  (row `debug-nested-submit`, ceiling 0). Read tools (`engine_state`, `demand_dispatch`,
  `mesh_stats`) become ordered `call`s (their bodies keep `engine_lock::with_engine_lock`) and
  queue behind a long evaluation — accepted, because "read after the write you awaited" is the
  point and the reads that must not wait are lock-free by construction.

### §4.8 Per-tool jobs and publication (ο, ρ) — OQ2; gui-state-sync D7 superseded

Every engine-touching debug tool is exactly ONE job (a job never submits to its own queue), so
each handler's multi-step flow (guard → write → diagnostics read) runs against one snapshot;
`ConfinedTarget::path_while_open` / `WriteTargetRefusal::DesignChanged` and every "harmless under
the serial-debug-ops assumption" caveat go with the interleaving they guarded.

| tool | job kind | job body (reused impl) | publishes | reply | frontend after the ticket | leaf |
|---|---|---|---|---|---|---|
| `reify_set_parameter` | stamped evaluation over `delegated_tool_job` | registry → host `set_parameter` (`commands::set_parameter_impl`) | committed state, or `restored` on refusal | typed response + `generation` | none — delta; the disk write reaches the editor through the watcher | ο |
| `reify_update_source` | stamped evaluation over `delegated_tool_job` | registry → host `update_source` (active-file guard, `commands::update_source_impl`) | committed state (none on compile failure) | typed response + `generation` | `sync_editor_buffer {path, content}` (new REST-only bridge handler replacing `apply_gui_state`'s `file` member) | ο |
| `reify_open_file` | stamped evaluation over `delegated_tool_job` | registry → host `open_file` + `get_source` | the host's produced state | typed response + `generation` | today's `open_file {path, content, guiState}` push built from the produced state (ο); ρ switches it to `published` with the open funnel | ο, ρ |
| `reify_save_file`, `reify_export` | `engine_call` over `delegated_tool_job` | registry → host `save_file` / `export` | nothing | typed response, no generation | none | ο |
| `open_file`, `load_fixture` | snapshot | canonicalize + `commands::load_initial_file_impl` (resolve after the lock) | the loaded state, generation g | the frontend's `{ok, path}` verbatim + `generation: g` | `open_file {path, content, published}` → bridge: `editor.openFile`, `engineStore.applyPublishedState` (generation-fenced), `viewState.resetToDefaultView` | ρ |
| `set_fea_case` | stamped evaluation | `commands::set_active_fea_case_impl` → `publish_only` (made `pub(crate)`) | yes | `{ok, case, generation}` | none — delta only | ρ |
| `engine_state`, `demand_dispatch`, `mesh_stats` | `engine_call` | as today | nothing | as today | none | ρ |
| `health`, `engine_status`, `wait_for_idle` | `activity` / `wait_until_settled` | lock-free | — | as today (+ `until_generation`) | `wait_for_idle`'s frontend half as today | π |

ο runs its jobs on today's `DebugServerState.evals` with the job closing over the engine `Arc`;
ρ swaps that wrapper for `QueuedEngine`. **Deleted by ο** (with the five handlers): every helper
only they call — the four `reify_*_on_engine_and_refresh_baseline` seam cores,
`write_on_engine_and_refresh_baseline`, the `reify_*` params/envelope helpers, `push_gui_state`,
`confined_write_target`, `WriteTargetRefusal::DesignChanged`/`ConfinedTarget::path_while_open`,
and the `debug_server.rs` originals of `update_source_target_matches_active`,
`resolve_update_source_push_path` and `filter_diagnostics_for_file` (ξ copied them into the
host). **Deleted by ρ:** `run_on_engine`, `open_source_into_engine_and_refresh_baseline`,
`set_fea_case_on_engine(_and_refresh_baseline)`, `write_tool_frontend_payload`,
`fea_case_frontend_payload`, the `apply_gui_state` bridge handler, the remaining
`compute_delta`-for-side-effect calls (`diff::compute_delta` loses its last non-test caller and
becomes `#[cfg(test)]` or goes), `DebugServerState.last_state`, the open funnel's double-read
`debug_assert_eq!` canary, and every serial-debug-ops restatement; ρ also corrects
`eval_queue.rs`'s `EvalProgress.outstanding` doc, which names `run_on_engine`/#7854.
`frontend_ok` stays: it checks the two frontend commands that remain (`open_file`,
`sync_editor_buffer`). fk-c4da71a0014e's swallowed refusal closes by removing the push it was
swallowed from. `EvalProgress.outstanding` now counts debug writes and `evaluation-status`
toggles around them (#7854's consequence (c)). Any debug tool added between authoring and ρ
(e.g. #6905's `set_purpose`) is converted by ρ too: the `debug-engine-handle` row admits no
`run_on_engine(`.

**gui-state-sync D7 superseded (Leo, OQ2, 2026-10-13).** D7 kept a parallel synchronous
`query_frontend` full-state push for three reasons; all three are answered. (1) *Sync point*: the
e2e harness synchronises on `wait_for_idle`, the same two-half wait every GUI-driven mutation it
drives already uses; debug writes now publish through `SnapshotPublisher` and every publishing
write replies with its `generation`, which `wait_for_idle({until_generation})` makes exact. The
property relied on: the `debug-request` event carrying `wait_for_idle`'s frontend half is emitted
after the lane published the write's delta (happens-before through the ticket), and one
`AppHandle` delivers events to one webview in emit order. (2) *Frontend-specific work*: true only
for the open funnel, which keeps a narrowed `open_file` command carrying the queue's
`PublishedState`; `apply_gui_state`'s "no view reset" property is simply what deltas do;
`reify_update_source`'s editor reconciliation is `sync_editor_buffer` with path and content only.
(3) *OCCT panics inside tokio*: the ENGINE lane is a dedicated OS thread
(`large_stack::post_to_worker`), so a queued job never runs inside tokio. ρ writes this paragraph
into `docs/prds/v0_6/gui-state-sync.md` §4 after D7 (that PRD's L6 primary route, taken) and into
`docs/debug-mcp-contract.md`.

### §4.9 Typed debug tool table and its export (τ); INV-GUI-2 (ο, ρ)

```rust
// gui/src-tauri/src/debug_tools.rs  (ungated, τ)
pub enum EngineTool { Health, EngineStatus, EngineState, DemandDispatch, MeshStats, MorphStats,
    MeshMorphStats, OpenFile, LoadFixture, SetFeaCase, WaitForIdle, WaitFor, WaitForSelector,
    Screenshot, ScreenshotWindow, ElementScreenshot }
pub enum FrontendSync { None, EditorBuffer, OpenDesign }
pub enum ToolRoute { Engine(EngineTool), Frontend, ReifyMcp(FrontendSync) }
pub struct DebugTool { pub name: String, pub description: String, pub input_schema: Value, pub route: ToolRoute }
pub const DEBUG_DELEGATED_TOOLS: [(&str, FrontendSync); 5];   // the five reify_* names, listed once
pub fn debug_tools(registry: &ToolRegistry) -> Vec<DebugTool>; // native rows + delegated rows from the registry
pub fn export_json(tools: &[DebugTool]) -> String;             // the committed gui/src-tauri/debug-tools.json
```

- `tool_defs()` (moved to `debug_protocol.rs` by #6964; its five `reify_*` rows rewritten by ο to
  copy `registry.tool(name)`) is replaced by `debug_tools`; its ~1,260 lines of schema literals
  live in their own module so `debug_protocol.rs` stays under the h14 alarm. Dispatch in
  `debug_server.rs` is an exhaustive `match` over `EngineTool`: a variant without a handler does
  not compile, and no name→arm string match remains. `Frontend` rows go to
  `FrontendPort::query(name, …)`; `ReifyMcp` rows go to `delegated_tool_job`; their description
  and schema are `registry.tool(name)` verbatim (one home).
- **Export.** `gui/src-tauri/debug-tools.json`, committed: `{ "tools": [{name, description,
  inputSchema, route: "engine"|"frontend"|"reify_mcp", frontend_handler: string|null}],
  "fixtures": {name: repo-relative path} }`. An ungated Rust test serializes the table (structs,
  not `Value` maps, so key order is declaration order) and compares bytes with the committed file,
  failing with the regeneration command (`REIFY_UPDATE_DEBUG_TOOLS=1` rewrites it). The GUI test
  script reads the committed file, so it needs no Rust build; the merge gate runs both halves, so
  drift reds.
- **Consumers.** τ repoints `gui/src/__tests__/toolDefNames.ts` (→ `debugToolTable()` over the
  JSON; `readDebugServerSource`/`extractToolDefNames`/`analyzeToolDefs` and their regex fixtures
  deleted), `debugParity.test.ts` (`PURE_ENGINE_SIDE` kept as a name, derived as
  `frontend_handler == null`; `extractDispatchArmNames` and its case deleted — the exhaustive
  enum is the guard), `sidecarPromptParity.test.ts` and `gui/test/visual/assertions.test.ts`
  (import lines only). S26 (G4 owner of `KNOWN_DEBUG_TOOL_NAMES` and the `system-prompt.ts`
  tables) later derives those, and `assertions.ts` `FIXTURES`, from the export; τ does not edit
  them.

**INV-GUI-2.** ο deletes `tests/debug_write_tool_routing_tests.rs` (1,194 lines),
`debug_write_tool_routing_fixtures.rs` (567) and `REIFY_INV_GUI_2_BYPASS` (their anchors vanish
with the handlers) and, in the same diff, rewrites the row's enforcement and Pointer so they name
no deleted file (S24 same-diff rule): interim "AI/MCP half: type-level via
`mcp_context::delegated_tool_job`; native debug writes: pending ρ". ρ writes the final row and
flips the status cell; ρ's id is ρ's entry in #8821's owner map (recorded at decompose). The
final row, in S24's Pointer grammar if #8821 has landed (PREGISTRY stays green), else with the
pointer inline in the enforcement cell:

> | INV-GUI-2 | Every engine-mutation entry point (GUI command, debug server, MCP/AI tools) flows
> through the same delta choke-point | **type** for the debug/AI/MCP half: the debug module
> reaches the engine only through `queued_engine::QueuedEngine`, whose jobs borrow the engine for
> their own duration on the ENGINE lane and whose only publisher is `SnapshotPublisher` (the sole
> production writer of the delta baseline); the module holds no `Arc<Mutex<EngineSession>>` and
> no baseline handle, so a second publication path does not compile. Reify-mcp tools run on the
> GUI only as `mcp_context::delegated_tool_job` under that capability. Rows
> `debug-engine-handle`, `debug-baseline-side-effect`, `debug-full-state-push`
> (`tests/infra/adoption-ratchet.manifest`) hold the counts at 0; `debug_server_tests::*` pin one
> generation and one delta per publishing write. GUI command half: `commands::*` build
> `EvalRequest`s on the same queue; FS watcher: `disk_reload_edit`. Residual: `AppState.engine`
> and the `pub commands::*_impl` seams remain reachable off-queue by new GUI-side code — enforced
> by convention there, not by type. Pointer: `item:gui/src-tauri/src/queued_engine.rs::QueuedEngine;
> test:gui/src-tauri/src/tests/debug_server_tests.rs` | partial: enforced(type) for the debug and
> AI/MCP entry points, convention for GUI commands and the FS watcher | gui-state-sync (core +
> GUI/watcher); S25 (debug + AI/MCP) |

INV-GUI-3's mechanism cell gains (ξ): "`TauriToolContext::set_parameter` →
`commands::set_parameter_impl` is the AI path's single write-back entry (the same as the property
panel)".

### §4.10 One AI tool contract: typed pairs over the host trait (ν, ξ, ο)

**Registry (ν).**

```rust
// crates/reify-mcp/src/registry.rs (ToolClass) and types.rs (ToolInfo)
pub enum ToolClass { Inline, EngineCall, EngineMutation }      // required at registration (#7722)
pub struct ToolInfo { pub name: String, pub description: String,
    #[serde(rename = "inputSchema")] pub input_schema: Value, #[serde(skip)] pub class: ToolClass }
impl ToolRegistry {
    pub fn standard() -> Self;                                   // new() + register_all_tools
    pub fn register<Req: DeserializeOwned, Resp: Serialize>(&mut self, name: &'static str,
        description: &'static str, input_schema: Value, class: ToolClass,
        handler: impl Fn(Req, &dyn ReifyToolContext) -> Result<Resp, ToolError> + Send + Sync + 'static);
    pub fn call_tool(&self, name: &str, params: Value, ctx: &dyn ReifyToolContext) -> Result<Value, ToolError>;
    pub fn class_of(&self, name: &str) -> Option<ToolClass>;
    pub fn tool(&self, name: &str) -> Option<&ToolInfo>;
    pub fn list_tools(&self) -> Vec<ToolInfo>;
}
```

- All 16 tools get a `Deserialize` request and a `Serialize` response (reads reuse the info types).
  The registry deserializes (a serde error is `ToolError::InvalidParams(<serde's message>)`),
  calls the typed handler, serializes. The 17 `params["…"]` hand-parses go to 0. All 16 are typed
  because the registry has one registration API; the eleven the debug server does not delegate
  keep #6665's KEEP status (substrate for a future headless host) and have no production
  consumer — stated, not hidden (as is `TauriToolContext`'s event emitter, which only the
  undelegated `focus_entity`/`navigate_to_source` use).
- Each schema is a hand-written `serde_json::Value` beside its request type, tied to the type by
  one table-driven round-trip test (BT8). `schemars` is rejected: no schema-derive crate exists in
  the workspace (`git grep -n schemars -- '*.toml'` is empty), its output carries
  `$schema`/`definitions` an MCP client does not expect, and an unverified new dependency on the
  crate every host links is a G3 risk a const plus a test does not carry.
- **`reify_set_parameter`'s grammar (OQ1) has one home**: `SetParameterRequest.value`'s schema
  text — a DIMENSIONED cell takes a unit-bearing literal in the spelling typed in source
  (`120mm`, `0.5m`, `45deg`), parsed exactly as the property-panel edit box parses it; a bare
  number is refused rather than read as an SI magnitude; a DIMENSIONLESS cell takes a bare number
  or `true`/`false`; the written-back literal keeps the replaced literal's unit. Enforcement is the
  host's existing `commit_parameter` → `apply_param_to_source_str` path. Known residue: a dimension
  no unit ladder covers (Money) still accepts a bare number — #7759 owns it. The sidecar prompt
  (`gui/sidecar/src/system-prompt.ts`, S26's) already describes this grammar, so no prompt edit is
  needed. ν's observable half is over `McpServer::handle_message` (the transport #6665 keeps);
  the sidecar sees the grammar and the `enum` lists in `tools/list` once ο delegates.
- Trait changes (host-visible): `get_diagnostics(&self, file: Option<&str>)` (the host owns its
  key scheme and matches with `engine::source_key_matches_path`; #7071);
  `update_source(..) -> Result<(), ToolError>` (`UpdateResult`, discarded today, is deleted);
  `SetParamResult { new_value: Option<String>, unit: Option<String> }` (`Ok` is success).
  `MockToolContext` follows.

**Host (ξ) and delegation (ο).**

```rust
// gui/src-tauri/src/mcp_context.rs  (ξ)
pub struct TauriToolContext<'e> { engine: &'e Mutex<EngineSession>, selection: Arc<RwLock<SelectionInfo>>,
    event_emitter: Option<EventEmitter>, produced: Mutex<Option<GuiState>> }
impl<'e> TauriToolContext<'e> {
    pub fn borrowing(engine: &'e Mutex<EngineSession>, selection: Arc<RwLock<SelectionInfo>>) -> Self;
    /// The GuiState the last mutating method built (committed, or the restored state of a
    /// refused commit) — taken, not cloned — for the one job that owns this context to publish.
    pub fn take_produced_state(&self) -> Option<GuiState>;
}
pub struct DelegatedReply { pub value: Value, pub produced: Option<GuiState>, pub active_path: Option<PathBuf> }
pub fn delegated_tool_job(registry: Arc<ToolRegistry>, selection: Arc<RwLock<SelectionInfo>>,
    name: String, params: Value) -> impl FnOnce(&Mutex<EngineSession>) -> EvalOutcome<DelegatedReply> + Send + 'static;
```

ξ builds the host and `delegated_tool_job`, and moves today's `mcp_tool_call_evaluation` onto it
(so the existing path exercises the host until ο deletes it). ο makes the debug server submit
`delegated_tool_job` as a stamped evaluation for an `EngineMutation` tool and as an
`engine_call` for an `EngineCall` tool, from one `handle_delegated` arm (a name that cannot
match `handle_reify_`). It publishes the state the host's mutation already built — no second
`build_gui_state`, unlike today's `mcp_tool_call_evaluation`. ο deletes the dead `mcp_tool_call`
Tauri command (main.rs definition and `generate_handler!` entry), `mcp_tool_call_impl`,
`mcp_tool_call_evaluation` and the fresh-registry-per-call; `ToolRegistry::standard()` is built
once at `spawn_debug_server`. ο also deletes the 31 `write_tools.rs` envelope/params/
schema-parity tests that #6964 moves into `debug_protocol_tests.rs` (their subject is now
reify-mcp's typed pair, pinned there) and `debug_server/tests/rebuild_cost.rs` (its subject, the
pure-I/O rebuild for the baseline, no longer exists).

**Placement of every behaviour the two implementations differ on** (R4 §2b; ξ unless noted) —
no safety behaviour is lost:

| # | behaviour today | after | user-visible consequence |
|---|---|---|---|
| 1 | set_parameter grammar: reify-mcp "bare SI", GUI/debug unit-bearing | reify-mcp schema text (ν, OQ1); host enforcement unchanged | one grammar on every surface |
| 2 | persistence to the `.ri` | host (`set_parameter_impl`; INV-GUI-3) | the description may say "rewrites the default literal on disk" |
| 3 | update_source active-file guard (debug only) | host `update_source`: ξ re-implements the guard in `mcp_context.rs`; ο deletes the `debug_server.rs` original and its tests | guards every reify-mcp caller |
| 4 | diagnostics filter (`==` on stem keys vs `source_key_matches_path`) | trait `get_diagnostics(file)` (ν); host matches | `reify_get_diagnostics {file_path}` filters correctly too (#7071) |
| 5 | update_source `success:true` hard-coded; `UpdateResult` discarded | `UpdateResult` deleted (ν); success set on `Ok` | none on the GUI host |
| 6 | write confinement (debug only; host unconfined) | host `save_file`/`export` call `write_confinement::confine_to_design_dir` inside the job (ξ); ο removes `DesignChanged`/`path_while_open` with `confined_write_target` | reify-mcp callers confined (#7181); `DesignChanged` variant removed |
| 7 | rejected-buffer save interlock; "no active file" refusal (debug only) | host `save_file` | same refusals on both surfaces |
| 8 | frontend push + baseline refresh (debug only) | deleted; the queue publishes (§4.8; ο, ρ) | — |
| 9 | open_file canonicalize + resolve + editor open; `path` alias | host `open_file` = `canonicalize_debug_open_path` + `load_initial_file_impl`; the `path` alias is dropped for `reify_open_file` (ο; debug-native `open_file` keeps `path`) | `reify_open_file {path}` is refused "missing field `file_path`" |
| 10 | save_file wrong-typed optional: debug errors, reify-mcp treats as absent | typed `Option<String>` (ν): serde refuses | debug behaviour wins on both surfaces |
| 11 | set_parameter cell absent after a landed write: debug nulls, host errors | nullable response fields | never report failure for a write that landed |
| 12 | error text (`Error: x is required` vs `invalid parameters: …`) | debug envelope unchanged; `<e>` is `ToolError`'s Display (ο) | a missing field reads `Error: invalid parameters: missing field \`file_path\`` (documented) |
| 13 | lock/poison policy | host uses `engine_lock::with_engine_lock` | a poisoned mutex no longer fails the surface permanently |
| 14 | per-call 256 MiB thread vs lane | the lane (ο for the five write tools; ρ for the rest) | `spawn_on_large_stack` has no debug caller after ρ |
| 15 | update_source failure: bare `update_source` (debug) vs `update_source_impl` (editor) | host uses `update_source_impl` | a rejected AI edit shows the hot-reload banner exactly as a rejected editor edit (deliberate) |

### §4.11 MCP wire enums and one `ValueCellKind` keyword (σ)

- reify-mcp depends only on reify-core, so the MCP wire vocabularies are reify-mcp's own serde
  enums, whose tokens are today's bytes: `ParameterKind { Param, Let, Auto }` (PascalCase; the
  `free` flag is not on the wire on the GUI host, and the CLI host that emitted `Auto(free)` dies
  with #6665 — a recorded narrowing), `Determinacy` (snake_case, four tokens), `ConstraintStatus`
  (lowercase, three tokens; the CLI's `"unknown"` dies with #6665), `EvalPhase { Idle, Evaluating }`.
  `DiagnosticInfo.severity` becomes `reify_core::Severity` (already serde PascalCase — same bytes);
  its producers are one site in reify-core and three in the GUI engine (S12 owns
  `reify_core::Diagnostic`'s shape; σ edits only the sibling `DiagnosticInfo`).
- The host maps by exhaustive `match` in `mcp_context.rs` from θ's GUI enums, replacing θ's
  interim `as_wire_str()` renderings (the orphan rule forbids
  `impl From<reify_compiler::ValueCellKind> for reify_mcp::ParameterKind` in the gui crate; a
  match is compile-checked both ways). The GUI and MCP enums share the name `ConstraintStatus`;
  `mcp_context.rs` imports reify-mcp's under an alias.
- `ValueCellKind::keyword()` (`param`/`let`/`auto`/`auto(free)`, the DSL spelling) lands on the
  enum in `crates/reify-compiler/src/types.rs`; `reify-lsp/src/hover.rs` uses it. After θ and σ
  the four renderers become three: `keyword()`; θ's `ValueKind` serde enum (`From<ValueCellKind>`,
  no string function); and `cell_kind_tree_str`, kept on purpose for the tree/identity vocabulary
  (§4.4).
- Filters compare enums: `GetConstraintsRequest.status: Option<ConstraintStatus>`,
  `GetDiagnosticsRequest.severity: Option<Severity>`; an unknown token is `InvalidParams` naming the
  accepted set, and the schemas' `enum` lists show the vocabulary in `tools/list`. The severity
  filter becomes exact PascalCase (a client sending `"error"` gets a refusal naming the three
  tokens — a deliberate, documented tightening).

### §4.12 The debug_server.rs partition (no separate leaf)

The record's six units are realised by #6964 plus ο/ρ/τ's deletions, not by a further cut: #6964
moves `tool_defs`, the pure transport helpers, port parsing, `fixture_relpath`, the morph handlers
and every test to the ungated `debug_protocol.rs`; ο and ρ delete the handlers and helpers §4.8
assigns to each; τ moves the table to `debug_tools.rs`. What remains gated in `debug_server.rs` — transport (`JsonRpc*`, `handle_mcp`,
`handle_rest`, `BoundDebugListener`, `spawn_debug_server`), state plus the exhaustive dispatch,
the read/wait/image handlers, the open funnel, `set_fea_case`, the delegation arm and
`frontend_ok` — is an estimated 700–900 lines from today's 3,365 non-test. Every import is
downward (`debug_server` → `debug_tools` → reify-mcp; `debug_server` → `debug_protocol`;
`debug_server` → `queued_engine`, `debug::FrontendPort`). The record's `engine_access.rs` remedy is
moot because `run_on_engine` is deleted and `DebugServerState` is four fields. Should the residual
exceed the h14 alarm when τ lands, τ records a partition or a stay-whole verdict in its task record.

## §5 — Boundary tests (two-way)

Engine half (GUI crate suite; `scripts/gui-test.sh` for the TS halves):

| # | test | asserts | direction |
|---|---|---|---|
| B1 | `case_switch_keeps_display_routing` (η, RED first) | on `tests/prd-gate/fixtures/s25_case_switch_display_routing.ri`, `set_active_fea_case("overload")`'s state equals the built state's `display_panes`, `display_appearance` and every mesh `appearance`; stress channels differ | forward: a rebuild path keeps derived fields |
| B1' | `case_switch_publishes_no_routing_delta` (η) | through `active_fea_case_evaluation` over `ManualQueue`, the delta has no `changed_display_panes`/`changed_display_appearance` | reverse: the frontend is not told to clear |
| B2 | `load_from_compiled_resets_like_commit_state` (ζ, RED first) | after `load_from_compiled`, `geometry_derived_cache` is empty and every cache equals what `load_from_source` of the same module leaves | forward: one reset list |
| B2' | `every_reload_entry_point_goes_through_reload` (ζ) | `load_from_source`, `load_file`, `update_source` each produce exactly one telemetry sequence (RecordingSessionEvents after λ) | reverse: no open-coded pipeline survives |
| B3 | `wire_enums_keep_their_bytes` (θ) | golden JSON for one `ValueData`/`ConstraintData`/`EntityTreeNode` per variant equals today's | forward: wire unchanged |
| B3' | `overlay_and_recheck_compare_typed_values` (θ) | `surface_geometry_derived_cells` and `recheck_indeterminate_constraints` produce today's results on the existing constraint-recheck fixtures; rows at 0 | reverse |
| B4 | `every_commit_reaches_telemetry` (λ) | the per-entry-point emitter tests, on `RecordingSessionEvents`, see one commit sequence per entry point; an ignored `CommitReceipt` is a compile error under `#![deny(unused_must_use)]` (rustc, every build; the gate's clippy `-D warnings` is the second net) | forward |
| B4' | `session_events_installed_once_reaches_every_channel` (λ) | one `RecordingSessionEvents` receives auto-resolve, fea-case, mode-shape, warm-pool, solve start/finish and solver progress from a fixture solve | reverse: no channel bypasses the sink |
| B5 | `fea_fields_ride_deltas` (μ) | a reload that changes FEA diagnostics publishes `fea-diagnostics-changed` through `delta_to_events`; a reload that clears convergence publishes `null`; an unchanged reload publishes neither | both (unchanged vs cleared) |

Debug/MCP half (headless over `ManualQueue`/`RecordingObserver`, plus `ScriptedFrontend:
FrontendPort` from π; handler tests in the gated `tests/debug_server_tests.rs` #6964 prescribes;
reify-mcp tests in its crate):

| # | test | asserts | direction |
|---|---|---|---|
| BT1 | `a_delegated_mutation_is_one_generation_and_one_delta` (ο) | `reify_set_parameter {"100mm"}` as a stamped evaluation: one generation g, reply stamped g, one delta carrying the cell, activity Evaluating→Idle | forward |
| BT1' | `a_delegated_call_issues_no_generation` (ο) | `reify_save_file {}` as `engine_call`: no generation, no delta, file written | reverse |
| BT2 | `a_refused_set_parameter_publishes_the_restored_state` (ξ) | refusal → reply `Err`, the job's outcome publishes the restored state | forward |
| BT2' | `a_rejected_update_source_publishes_nothing` (ξ) | uncompilable text → `Err`, no publish, `holds_rejected_source`, next `reify_save_file` refused | reverse |
| BT3 | `save_as_is_confined_inside_one_job` (ξ) | outside the design dir → `OutsideDesignDir`, no file; inside → written | forward |
| BT3' | `confinement_tracks_the_design_opened_before_it` (ο) | queued `reify_open_file B` then `reify_save_file` under A, no drain between → refused against B | reverse: ordering replaces `DesignChanged` |
| BT4 | `update_source_syncs_the_editor_through_the_port` (ρ) | one `sync_editor_buffer {path, content}`; reply has `generation`; diagnostics filtered | forward |
| BT4' | `set_parameter_makes_no_frontend_call` (ρ) | zero frontend calls, one delta | reverse |
| BT4'' | `a_refused_editor_sync_is_reported_after_the_delta` (ρ) | scripted `{error}` → tool `Err`, delta still published | forward |
| BT5 | `the_open_funnel_pushes_the_published_state` (ρ) | one `open_file {path, content, published: {generation: g, state}}`, g = the issued generation; reply stamped | forward |
| BT5' | `an_unknown_fixture_submits_nothing` (ρ) | `Err`, nothing queued, no frontend call | reverse |
| BT6 | `tools_list_serves_the_registry_rows_verbatim` (ο) | the five delegated rows equal `registry.tool(name)`; none `Inline` | forward |
| BT6' | `undelegated_registry_tools_are_not_advertised` (ο) | the other eleven registry names are absent from the debug tool list | reverse |
| BT7 | `debug_tools_export_is_current` (τ) | serialized table == committed `gui/src-tauri/debug-tools.json` (no `--features gui`) | forward |
| BT7' | `export_detects_a_changed_description` (τ) | an in-memory change differs (positive control) | reverse |
| BT8 | `every_schema_agrees_with_its_request_type` (ν, all 16) | each `properties` key deserializes; each `required` key's absence → `InvalidParams` naming it; optional absence OK; each `enum` list = the serde tokens | both |
| BT9 | `constraint_status_filter_is_typed` (σ) | `"bogus"` → `InvalidParams` naming the set; `"violated"` → only Violated rows | both |
| BT10 | `activity_answers_while_a_job_holds_the_lane` (π) | `QueuedEngine::activity()` reports busy without waiting while a job is parked | forward: probe stays lock-free |

Frontend (vitest `debugContract.test.ts`): ο adds `sync_editor_buffer` (refuses a malformed
payload before touching the editor); ρ makes the `open_file` handler apply `published` through
`applyPublishedState` (a stale generation is skipped, the tab still opens) and deletes
`apply_gui_state`. Visual harness (ρ's signal): the `set_fea_case` → `wait_for_idle` → screenshot
scenarios stay green on the delta path.

## §6 — Resolved design decisions

- **D1 — engine.rs stays the module root.** Child modules live under `gui/src-tauri/src/engine/`;
  every pending task and delivered check that names `gui/src-tauri/src/engine.rs` still names the
  session core. Rejected: `engine/mod.rs`.
- **D2 — `keys.rs` split from `units.rs`** (heuristic *well-defined purpose for each entity*).
- **D3 — δ moves `scalar_to_f64` into `engine/mechanism.rs`** instead of waiting on #8698's chain;
  #8698's check is widened to follow it (§8).
- **D4 — the `#[cfg(test)] impl EngineSession` block stays with `EngineSession`**; λ deletes the
  emitter drivers, ζ the hand-copied resets (§2.2).
- **D5 — `reload()` over a named `CompiledSource`, before #5519 (OQ4).**
- **D6 — one post-tessellation builder; the cold-start and preview literals stay.**
- **D7 — wire enums keep wire bytes with per-variant renames (OQ3)**; θ makes no TS edit;
  `EntityTreeNode.kind`/`cell_kind_tree_str` stay (fingerprint stability).
- **D8 — eval Value shapes decoded once per shape in `engine/result_views.rs`**; the finish
  mapping's home is `display_routing.rs`; the mechanism goes through S6's `MechanismTree` (κ).
  ElasticResult's eventual single home is S15's decoder (#8690) — breadcrumb in ι.
- **D9 — `SessionEvents` is session-level, separate from `EvalObserver`** (queue-level, no
  session handle): the precedent shape, not the thing to extend.
- **D10 — telemetry is enforced by a `#[must_use]` `CommitReceipt` plus a crate-level
  `#![deny(unused_must_use)]` that λ adds.** Rejected: a Drop-time `debug_assert!` (runtime,
  debug-only); a compile-fail doc-test (the gate runs no doc-tests).
- **D11 — `fea_diagnostics`/`fea_convergence` onto deltas (μ) before debug writes go delta-only
  (ρ ← μ)**; a `diffed whole_opt` arm for `Option<T>`; event names and payloads kept
  (gui-state-sync OQ2's "fold later").
- **D12 — build on #6964's cut** (Leo's D3/D5 and esc-7240-11 for debug-server-gating-boundary
  stand): every S25 leaf editing `debug_server.rs` depends on #6964; no separate debug-server
  split leaf (§4.12).
- **D13 — `QueuedEngine` is the engine-access capability, in its own module** (§4.7). Rejected: a
  `WriteSeam` token minted by `write_on_engine_and_refresh_baseline` (proves "went through the
  seam", not "went through the queue"); the engine `Arc` inside `EvalQueue` (binds an
  engine-agnostic queue to one engine); `QueuedEngine` inside `eval_queue.rs` (an SCC with
  `engine_activity`).
- **D14 — per-tool publication with stamped replies (OQ2)**, `until_generation` on
  `wait_for_idle`; debug writes are ordered barriers, never coalescing edits (a debug caller has no
  `EditOrder`, and coalescing an AI's writes would resolve some unrun).
- **D15 — delegation through one `delegated_tool_job`** with every behaviour difference placed
  (§4.10); only the five ruled names are delegated.
- **D16 — typed pairs for all 16 tools, hand-written schema beside each type, `ToolClass`
  required at registration.**
- **D17 — MCP wire enums in reify-mcp; `keyword()` in reify-compiler.**
- **D18 — the debug tool table in `debug_tools.rs`, exported to a committed JSON with a
  regenerate-and-diff test** — the same shape S26 ruled for `bindings.ts`.
- **D19 — adoption ratchets are rows in `tests/infra/adoption-ratchet.manifest`** (the repo-wide
  engine S19 α #8678 landed), each added by the leaf that extracts, at its post-extraction count
  and named in its done criteria (INV candidate 5; Appendix A). `adoption_ratchet.py` counts every
  line whose first non-blank character is not `#` — Rust `//` comments included — so a ceiling-0
  row also forbids prose that spells the pattern; leaves word their comments accordingly.
- **D20 — ξ and ο are separate leaves, as are π and ρ** (critic C1-06): the host half needs no
  #6964, and the capability lands before the rewiring that uses it.

## §7 — Rulings

Program ruling (Leo, 2026-10-12) as quoted in the header. Session rulings (Leo, on questions this
session put):

- **OQ1 (2026-10-13) — `reify_set_parameter` value grammar → unit literal.** A dimensioned cell
  takes a unit-bearing literal; a bare number is accepted only for a dimensionless cell; reify-mcp's
  schema text is corrected to match (§4.10).
- **OQ2 (2026-10-13) — debug publication after the queue → per tool.** `set_fea_case` and
  `reify_set_parameter` delta only; save/export publish nothing and carry no generation;
  `reify_update_source` delta + narrow editor sync; the open funnel replies with the
  `PublishedState`, applied generation-fenced, plus tab and view; every *publishing* write reply
  carries its generation; `wait_for_idle(since/until_generation)` is the sync point. Supersedes
  gui-state-sync D7 (§4.8).
- **OQ3 (2026-10-13) — closed-set wire-field enums → S25, wire-identical** (θ; subsumes #7471's
  Rust half). S26 generates TS unions from them and converts the remaining closed-set fields.
- **OQ4 (2026-10-13) — `reload()` vs #5519 → reload() first.** No edge forced on #5519; a
  cross-reference tells its implementer the producer swap is one site.
- **OQ5 (2026-10-14) — other tasks' delivered checks this PRD flips → this session corrects
  them** at decompose (three writes each: task `metadata.delivered_checks`, the owning PRD's
  `.yaml` and `.md` twins), landed in the second docs commit; S15's #8698 goes to the lead as a
  cross-reference (this session does not edit another stream's PRD). List in §8.

Lead decisions within those rulings (not put to Leo; recorded so a later reader can contest them):
D3, D12, D13's module, D19, D20; subsumptions #8595, #8600, #7471, #7722, #7071, #7181, #7854;
#6843 ordered, not subsumed (critic C1-07).

## §8 — Pre-conditions for activating

- Committed with this PRD: `tests/prd-gate/fixtures/s25_case_switch_display_routing.ri`
  (grammar gate: `tree-sitter parse --quiet` exit 0, 0 ERROR/MISSING nodes; `reify check` exit 0,
  "All constraints satisfied.", 2026-10-13 on `target/debug/reify` built 2026-10-12). Unregistered:
  η registers it in the rust-coupled fixture list wherever that lives when η lands
  (`_RUST_COUPLED_RI_FIXTURES` in `scripts/verify.sh` today; `tests/infra/test_verify_scope.sh`'s
  PG-DRIFT reds an unregistered Rust-read fixture).
- Upstream tasks (real edges, §10): #8680 (α), #8838/#8839/#8845 (κ), #6665 (ν), #6964 (ο, π,
  ρ, τ).
- **Other tasks' delivered checks corrected at decompose (OQ5)** — each a widening or re-point
  that is green before and after S25:

  | task | check | correction |
  |---|---|---|
  | #6723 (done; 8 pending dependents) | `"satisfied"` present in `engine.rs` | paths += `gui/src-tauri/src/types.rs` (θ's per-variant rename keeps the literal) |
  | #6794 | `fn diagnostics_to_info` present in `engine.rs` | paths += `gui/src-tauri/src/engine/projection.rs` |
  | #6732 | `fn tool_defs` present in `debug_server.rs` | pattern `fn (tool_defs|debug_tools)`; paths += `debug_protocol.rs`, `debug_tools.rs` |
  | #6833, #6905 | `fn run_on_engine` present in `debug_server.rs` | pattern `fn run_on_engine|pub struct QueuedEngine`; paths += `debug_protocol.rs`, `queued_engine.rs` |
  | #6833, #6905 | `name: "set_purpose"` present in `debug_server.rs` | paths += `debug_protocol.rs`, `debug_tools.rs` |
  | #6833, #6905 | `compute_delta` present in `debug_server.rs` | pattern `compute_delta|SnapshotPublisher`; paths += `eval_queue.rs` |
  | #6833, #6905 | `PurposeReflectiveAggregation` present in `engine.rs` | paths += `gui/src-tauri/src/engine/projection.rs` |
  | #6741, #6898 | `fn demand_dispatch_on_engine\(` present in `debug_server.rs` | pattern `fn demand_dispatch_on_engine\(|fn demand_dispatch_json`; paths += `debug_protocol.rs`, `commands.rs` |
  | #8698 (S15, lead) | `fn scalar_to_f64` absent in `engine.rs` | paths += `gui/src-tauri/src/engine/mechanism.rs`, `gui/src-tauri/src/engine/tensegrity.rs` |

  (Several of these are also flipped by #6964's own move into `debug_protocol.rs`; the widening
  covers both.) Source: a scan of all 724 tasks carrying delivered checks at `cf59570562`.
- No grammar work: `grammar_confirmed = true` on every leaf.

## §9 — Cross-PRD relationship and seam owners

| seam | owner | this PRD's relation |
|---|---|---|
| `gui/src-tauri/src/eval_queue.rs` `SnapshotPublisher` (+ `Stamped`, ο) | **S25** (G4) | ο extends it; #7854 subsumed (ρ) |
| `gui/src-tauri/src/queued_engine.rs` `QueuedEngine` (new) | **S25** | π creates it |
| `docs/debug-mcp-contract.md` and the sidecar allowlist `mcp__reify-debug__*` | **S25** (G4) | ο, ρ, τ rewrite the AI-write-tool, engine-activity, two-seams, defining-a-tool and drift-guard sections; the allowlist is unchanged |
| reify-mcp `ReifyToolContext` trait | **S25** (G4) | ν changes it after #6665 leaves one host |
| `gui/src-tauri/src/types.rs` as the generation source | S26 (G4; shared) | θ adds five wire-identical enums; λ/μ fix the event set; S26 generates from them |
| `KNOWN_DEBUG_TOOL_NAMES`, `system-prompt.ts` tool tables | S26 (G4) | not edited; τ's export is what S26 derives them from |
| INV-GUI-2 status cell + `docs/invariants.md` Pointer | S24 ξ #8821 (Pointer grammar, owner map) | ο repoints the Pointer when it deletes the scanner; ρ flips the status; ρ's id recorded on #8821 at decompose |
| PLINECITE `--rename` / Pointer same-diff obligations | S24 (#8822, #8821) | every S25 leaf that moves a file under PLINECITE scope or a symbol a Pointer names carries them (§10 preamble) |
| `docs/prds/debug-server-gating-boundary.md` α (#6964) | that PRD (Leo-ruled D3/D5) | consumed: S25's debug-server leaves depend on it; ο/ρ delete items #6964 moves (`write_tool_frontend_payload`, the seam cores, the write_tools tests) — a note on #6964 says so |
| `docs/prds/v0_6/gui-state-sync.md` D7 / L6 | that PRD | superseded by OQ2 (§4.8); ρ writes the supersession paragraph there |
| `docs/prds/v0_6/ai-native-editing.md` D3 (MCP home = reify-debug) | that PRD | unchanged; ρ updates its D4/§6.2 pointer ("the choke-point is the queue's publisher") |
| resolution-unification ε (#5519) compile twin, stdlib literal | resolution-unification | OQ4: no edge; cross-reference |
| S3 κ (#8680) `set_observed_demand` | S3 | α depends on it |
| S15 θ (#8698) `scalar_to_f64`; S15 ε (#8690) ElasticResult decoder | S15 | δ moves `scalar_to_f64`, #8698's check widened (lead); ι breadcrumbs #8690 |
| S6 `MechanismTree` (S6 ρ #8845), `JointKind` (S6 β #8829), GUI walker (S6 λ₀ #8838, S6 λ #8839) | S6 | κ depends on #8845, #8838, #8839; S6 λ₀/λ edit the mechanism unit δ moves (file lock; whoever lands second rebases, symbol-cited) |
| S8 ι #8907 `Value::Field` destructures in `engine.rs` | S8 | file lock, no edge |
| `reify_core::Diagnostic` shape | S12 (G4) | σ edits only the sibling `DiagnosticInfo.severity`; ordered against any S12 lock on `crates/reify-core/src/diagnostics.rs` |
| reify-mcp `jsonrpc.rs` (#8765), `language_chunks.rs` (#8763) | S27 | disjoint files; order-free |
| INV-ARCH-1 (module SCCs only shrink) | S3 α (#8673) | the new top-level modules (`queued_engine`, `debug_tools`) import downward; `assert-module-scc.py` measures top-level modules, so downwardness among `engine/*` children is enforced by review (and by α–ε's move-only rule), not by INV-ARCH-1 |

## §10 — Decomposition plan

Labels Greek; task ids filed 2026-10-14 (pass 2); no held leaf. Gate #8612 depends on ε #8941 and ο #8951. Every leaf carries
`metadata.x_finding_key` (list), `metadata.x_finding_run = "review-all-reify-20261010"`,
`metadata.source = "review-all"`, priority = the highest severity among its keys (medium where it
carries none), `grammar_confirmed = true`. "Rows" names the Appendix A ratchet rows the leaf adds
to `tests/infra/adoption-ratchet.manifest` at its post-extraction count (named in its done
criteria). Same-diff obligations on every leaf that moves code (S24): after #8822 lands, a moved
file under PLINECITE scope runs `plinecite-baseline-gen --rename <old>=<new>`; after #8821 lands, a
moved or renamed symbol named by a `docs/invariants.md` Pointer has its Pointer updated.

**Phase 1 — engine.rs partition (move-only; each: GUI suite green with tests moved, no test body
edited beyond `use` lines; no shim; tracing targets verbatim)**

- **α #8937 — `engine/units.rs`, `keys.rs`, `line_col.rs`, `write_atomic.rs`, `persistent_cache.rs`**
  and their tests (engine_tests unit 3 + the line-offset sections of unit 4); the `reify_core`
  re-export shim deleted, 17 test refs repointed. Subsumes **#8595**. Depends: #8680. Signal:
  `cargo test -p reify-gui` and `scripts/gui-test.sh` green;
  `git grep -n "fn parse_value_string_for_cell" gui/src-tauri/src/engine.rs` empty. Keys: none
  (contributes to fk-05311be442e6). medium.
- **β #8938 — `engine/fea_channels.rs`** + its tests (~30 from unit 7 + the integration block).
  Subsumes **#8600**. Depends: α. medium.
- **γ #8939 — `engine/display_routing.rs` (with `impl MergedTraitDefs`, inline style tests) and
  `engine/entity_tree.rs`** + tests. Depends: β. medium.
- **δ #8940 — `engine/mechanism.rs` (with `scalar_to_f64`, `pub(super)`) and `engine/tensegrity.rs`**
  + tests. Depends: γ. medium.
- **ε #8941 — `engine/projection.rs` (unit 2 + the expression printer)** + tests; `engine.rs` holds only
  the session core (§4.1); `engine_tests.rs` holds only session-core tests, split by concern if its
  residual exceeds the h14 alarm (measurement recorded in the task). Signal: as α, plus
  `git grep -nE "^(pub(\(crate\))? )?fn (build_values|apply_fea_channels|collect_display_routing)" gui/src-tauri/src/engine.rs`
  empty. Depends: δ. Keys: **fk-05311be442e6** (high, gate-dep #8612).

**Phase 2 — session internals**

- **ζ #8942 — one `reload()`; `CompiledSource`; one compile-error filter** (§4.2). B2/B2' RED first.
  Rows `gui-reload-pipeline`. Depends: none (OQ4). Keys: **fk-2dcacd59c79d**. medium.
- **η #8943 — one post-tessellation builder `assemble_gui_state`; the case switch keeps routing and
  appearance** (§4.3). B1/B1' RED first on the committed fixture (registered in the rust-coupled
  fixture list in the same diff; a verify-pipeline file, so full gate). Rows `gui-state-literal`.
  Signal also debug-MCP observable: on the fixture, `set_fea_case overload` then `engine_state`
  shows the same panes as before. Depends: ε. Keys: **fk-b28eb5276628**. medium.
- **θ #8944 — closed-set wire fields as serde enums (per-variant renames); typed overlay and recheck**
  (§4.4). B3/B3'. Deletes the `constraintVerdictTokens` scraper; interim `as_wire_str()` at the
  three `mcp_context.rs` readers. Subsumes **#7471** (Rust half). Rows
  `gui-dto-status-string-compare-root`, `-modules`. Depends: ε. Keys: **fk-995c30a8a446**. medium.
- **ι #8945 — typed result views; one finish mapping** (§4.5). Rows `gui-value-type-name-test-root`,
  `-modules`, `gui-value-field-get-root`, `-modules`, `gui-finish-mapping-copy`. Breadcrumb to
  #8690. Depends: β, γ, δ, ε. Keys: **fk-55213deb2362** (low). low.
- **κ #8946 — mechanism descriptor through S6's `MechanismTree` and `JointKind`** (§4.5). Rows
  `gui-mechanism-map-key-root`, `-modules` (ceiling 0). Depends: ι, **#8845**, **#8838**,
  **#8839**. Keys: none (contributes to fk-55213deb2362). low.

**Phase 3 — one session event sink**

- **λ #8947 — `SessionEvents`, one install, `CommitReceipt`, `#![deny(unused_must_use)]`** (§4.6).
  B4/B4'. Rows `gui-session-event-setter` (ceiling 0). Updates `docs/gui-event-channels.md`
  inventory. Depends: ζ. Keys: **fk-4ecb76174487**. medium.
- **μ #8948 — `fea_diagnostics`/`fea_convergence` onto deltas; `diffed whole_opt` arm** (§4.6). B5.
  Depends: λ, η. Keys: **fk-08fd82603d2b**. medium.

**Phase 4 — one AI tool contract**

- **ν #8949 — reify-mcp typed request/response, schema and `ToolClass` per tool** (§4.10). BT8. Rows
  `mcp-hand-parsed-params` (ceiling 0). Subsumes **#7722**, **#7071**. Signal: over
  `McpServer::handle_message`, `tools/call reify_save_file {"file_path": 120}` returns `isError`
  with serde's type error instead of saving the active file; `tools/list` shows the unit-literal
  grammar and the filter `enum`s. Depends: **#6665**. Keys: none (contributes to
  fk-bb84d4272cfd). medium.
- **ξ #8950 — host behaviours on `TauriToolContext<'e>`; `delegated_tool_job`** (§4.10 host, placement
  rows 2–7, 9 host half, 11, 13, 15). BT2/BT2'/BT3 against the real host over `ManualQueue`;
  `mcp_tool_call_evaluation` moved onto `delegated_tool_job`; INV-GUI-3 mechanism cell. Subsumes
  **#7181**. Signal (intermediate; the user-facing half arrives with ο): the host behaviour tests
  green with the engine and fixtures real, and the existing debug write-tool tests unchanged (ξ
  copies the guard and path helpers into the host and edits no `debug_server.rs` code, so it needs
  no #6964). Depends: ν. Keys: none (contributes to fk-bb84d4272cfd). medium.
- **ο #8951 — the debug server delegates the five `reify_*` tools; stamped evaluation; `mcp_tool_call`
  deleted; the INV-GUI-2 scanner retired** (§4.8 ο rows, §4.9 INV-GUI-2 interim, §4.10). Adds
  `Stamped<T>` and the stamped evaluation kind to `eval_queue.rs` (+ `eval_queue_tests.rs`);
  rewrites `tool_defs`'s five `reify_*` rows to copy `registry.tool(name)`; keeps five literal
  dispatch arms (`"reify_x" => handle_delegated(state, "reify_x", params)`) because
  `debugParity.test.ts` check (g) requires a literal arm per engine-side tool until τ retires it;
  adds the `sync_editor_buffer` bridge handler (with its `debugContract.test.ts` case — the
  headless port tests BT4–BT4'' are ρ's because `ScriptedFrontend` lands in π); `reify_open_file`
  keeps today's `open_file` push payload until ρ; deletes the five handlers and the helpers listed
  as ο's in §4.8, the 31 moved `write_tools.rs` tests and `rebuild_cost.rs`; contract-doc "AI
  write tools" rewritten; interim INV-GUI-2 row and Pointer. BT1/BT1'/BT3'/BT6/BT6'. Signal: in a `REIFY_DEBUG=1` launch `tools/list` shows the five with
  reify-mcp's descriptions verbatim; a successful `reify_set_parameter` replies with `generation`
  and `engine_status.generation` advances by one; `reify_save_file {"file_path": "/tmp/x.ri"}` is
  refused with the confinement rule; the GUI's `evaluation-status` toggles around an AI write.
  Depends: ξ, **#6964**. Keys: **fk-bb84d4272cfd** (high, gate-dep #8612), **fk-58f6f0d58857**.
- **σ #8952 — MCP wire enums; `ValueCellKind::keyword()`; typed filters** (§4.11). BT9. Depends: ν, θ,
  ξ, ε. Keys: **fk-cc4329a75e3e**. medium.

**Phase 5 — debug writes on the queue**

- **π #8953 — `QueuedEngine` (`queued_engine.rs`), `FrontendPort`, `until_generation`** (§4.7). BT10;
  `ScriptedFrontend` test double. Rows `debug-nested-submit`. Signal: `wait_for_idle({until_generation:
  g})` over debug MCP returns `{ok, generation ≥ g}`; `assert-module-scc.py check` green. Depends:
  ο, **#6964**. Keys: none (contributes to fk-7212abff873d). medium.
- **ρ #8954 — every debug engine access through `QueuedEngine`; open funnel, `set_fea_case` and reads
  rewired; gui-state-sync D7 superseded** (§4.7, §4.8 ρ rows). BT4–BT5', the vitest handler tests.
  Deletes the helpers listed as ρ's in §4.8 and corrects the `EvalProgress.outstanding` doc;
  switches the open funnel and `reify_open_file` pushes to `published`.
  Rows `debug-engine-handle`, `debug-baseline-side-effect`, `debug-serial-ops-assumption`,
  `debug-full-state-push`, `debug-full-state-push-bridge`. Final INV-GUI-2 row and status flip
  (#8821 owner map); D7 supersession paragraph in gui-state-sync.md and the contract doc;
  ai-native-editing D4/§6.2 pointer. Subsumes **#7854**. Signal: `open_file` / `load_fixture` /
  `set_fea_case` replies carry `generation`; during a debug `open_file` of a long design
  `engine_status` reports `queue_outstanding: 1`; the visual harness's `set_fea_case` →
  `wait_for_idle` → screenshot scenarios stay green. Depends: π, μ, **#6964**. Keys:
  **fk-7212abff873d**, **fk-c4da71a0014e**, **fk-484c81d71180**. medium.
- **τ #8955 — typed debug tool table and committed JSON export; regex-over-Rust parity retired**
  (§4.9). BT7/BT7'. Rows `debug-tool-name-regex-src`, `debug-tool-name-regex-visual`. Signal:
  `tools/list` equals the committed `gui/src-tauri/debug-tools.json` row for row, and renaming a
  tool without regenerating reds `debug_tools_export_is_current` in the ordinary workspace pass;
  `scripts/gui-test.sh` passes without a Rust build. Depends: ρ, ν, **#6964**. Keys:
  **fk-e2b409ec7580** (weakened; scope per §1). medium.

**Close**

- **ω #8956 — PRD close stamp.** Depends on every other leaf (a cancelled sibling counts as satisfied).
  Deliverable: the terminal Status marker naming the landed leaf ids, the AS-AUTHORED paragraph and
  LIVE/AS-AUTHORED map here and in the capability manifest (overlay "PRD terminal status"). low.

Edges added at decompose onto tasks outside the batch (their premise changes): #7716 and #8574 ←
ρ; #8074, #5101 and #7615 ← ο; #6843 ← θ. File-collision-only tasks (#6552, #8137, #8471, #7857,
#6407, #6442, #8185, #7892, #7835, #6564, #8298, #6905, #8907, #6519, #6961, #8286, S6 λ₀/λ) are
left to file locks. Subsumed and cancelled at filing (reason names the leaf): #8595 → α, #8600 → β,
#7471 → θ (TS half → S26), #7722 and #7071 → ν, #7181 → ξ, #7854 → ρ. Notes appended to #8821
(S25 owner map: INV-GUI-2 → ρ #8954), #6964, #5519, #6905 and #6843.

## §11 — Out of scope

- The GUI-side half of INV-GUI-2 (`AppState.engine`, `pub commands::*_impl`): convention there,
  stated as the residual (§4.9).
- A live-source accessor on `EngineSession` (#8074's rebuild for save/export): ordered after ο.
- #7716's return-on-enqueue and health starvation; #6964 itself; #6665 itself.
- `EntityTreeNode.kind`, the TS unions, and the remaining closed-set fields (phase, tensegrity
  kind, …): S26.
- `gui/src/debug/bridge.ts`'s partition (#7858, #8184) and schema-validated handlers: S26.
- The Engine constructor and `EngineConfig`: S1 (no S25 leaf touches `Engine::new` sites).
- Making the eleven undelegated reify-mcp tools reachable from the GUI.

## §12 — Open (tactical) questions

1. ο: whether the stamped kind is a `stamped_evaluation` sibling of `EvalRequest::evaluation` or
   `evaluation` itself replying `Stamped<T>` with the Tauri `set_active_fea_case` command mapping
   to `()` — the command's reply type must not change (S26 generates it).
2. ν: `reify_get_eval_status` on the GUI host stays a constant `Idle` with no surface serving it
   after ο; deleting the tool touches the "16 tools" docs and the chunk tests (#8763). ν may
   propose it in its task record; not done here.
3. ε: whether the residual `engine_tests.rs` needs a session-core split (measure at ε).
4. τ: the export's pretty-printer key order (serialize structs, not `Value` maps).
5. κ: whether `MechanismTree` exposes per-joint axis and range; if not, the read-only accessor
   κ adds (§4.5).
6. If a visual scenario flakes on a `set_fea_case` screenshot after ρ, the mitigation is a
   `published {generation}` marker so `wait_for_idle`'s frontend half waits on application rather
   than emission order — a ρ follow-up, not designed here.

## Gate walk (author mode)

- **G1 consumer named.** The partition and reload/builder/enums → the GUI frontend through the
  published state and the next agent's change; `SessionEvents` → the frontend's existing listeners
  (unchanged names/payloads) and S26; `QueuedEngine` → the debug server's handlers, the sidecar
  and the visual harness; typed reify-mcp pairs → the debug server's five delegated tools and
  #6665's future headless host (eleven, stated); the export → debugParity/toolDefNames now and S26
  later.
- **G2 user-observable signal.** Each leaf's signal is a GUI-state or debug-MCP observable, a
  `tools/list`/`tools/call` reply, or a behaviour test on the real engine; the partition leaves'
  signal is the GUI suite green with tests moved unedited; ξ and π are named intermediates whose
  user signal arrives with ο and ρ.
- **G3 substrate verified.** Anchors re-read at `b1a7be2982` by six reader seats, the design seat
  and the critic (~60 claims spot-checked); unchanged at `cf59570562`. `compile_program` is
  designed, not landed (OQ4); no schema-derive crate exists (D16); the adoption-ratchet engine
  exists (#8678); `#![deny(unused_must_use)]` is absent and λ adds it; the fixture passes the
  grammar gate and `reify check`; S6's `MechanismTree` is filed (#8845).
- **G4 seams.** §9, with owners from the program's G4 table; S25's three G4 artefacts are edited
  only by S25 leaves; S26's two are not edited.
- **G5 approach.** B + H: contracts §4 and two-way boundary tests §5.
- **G6 premises.** §2 records the corrections, including the critic's (§2.4's push premise,
  `QueuedEngine`'s SCC, the absent deny attribute); the weakened finding is re-scoped (§1); OQ1's
  Money residue is named (§4.10); the delivered checks this PRD would flip are corrected (§8).
- **G7 design invariants.** *nothing-vacuous-and-unowned*: the eleven undelegated tools, the event
  emitter and the constant `reify_get_eval_status` are stated with owners (§4.10, §12).
  *INV-SF-6 diagnostics-carry-codes*: no new Diagnostic is introduced. *INV-ARCH-1*: `QueuedEngine`
  homed to avoid an SCC (§4.7). INV-GUI-1 keeps its compile-time classification (μ adds an arm,
  not an escape). INV-GUI-2 moves from a lexer to a type (§4.9).
- **Drift-guard registration.** No new gate-resident binary or `tests/infra` member: ratchet rows
  join the existing manifest; new Rust tests join existing binaries and `tests/debug_server_tests.rs`
  (registered through `tests/mod.rs`); η registers its fixture in the same diff.
- **Docs-truth gate.** No `.ri` language surface changes. The AI tool surface's contract is
  `docs/debug-mcp-contract.md`, rewritten by ο/ρ/τ in the same diffs; the sidecar prompt already
  states OQ1's grammar.
- **META.** Decompose-and-queue without oversight yields one engine.rs of session code over
  downward-only modules, one publication path for every debug and AI write, one implementation
  per AI tool with one grammar, and one event sink — coherent with the program's rulings and with
  the PRDs whose decisions it consumes or supersedes (§9).

## Appendix A — Adoption-ratchet rows

Manifest grammar `pattern-id | ERE | path | ceiling | owner` (`tests/infra/adoption-ratchet.manifest`
header). "Today" is `git grep -E -c` at `cf59570562`, comment lines included (D19). Each owner leaf
adds its row(s) at the post-extraction count, which must be ≤ the ceiling column; a path glob that
does not exist yet is added by the leaf that creates it.

| pattern-id | ERE | path | today | ceiling | owner |
|---|---|---|---|---|---|
| gui-reload-pipeline | `check_with_solve_slot\(` | `gui/src-tauri/src/engine.rs` | 5 | 2 (definition + reload's call) | ζ |
| gui-state-literal | `(Ok\(\|= \|^ *)GuiState \{` | `gui/src-tauri/src/engine.rs` | 4 | 2 (cold start + builder) | η |
| gui-dto-status-string-compare-root | `(determinacy\|freshness\|status) == "\|== satisfaction_token` | `gui/src-tauri/src/engine.rs` | 4 | 0 | θ |
| gui-dto-status-string-compare-modules | same | `gui/src-tauri/src/engine/*.rs` | (after ε) | 0 | θ |
| gui-value-type-name-test-root | `type_name == "` | `gui/src-tauri/src/engine.rs` | 11 | 0 | ι |
| gui-value-type-name-test-modules | same | `gui/src-tauri/src/engine/*.rs` | (after ε) | 0 | ι |
| gui-value-field-get-root | `get\(&?"` | `gui/src-tauri/src/engine.rs` | 41 | post-ι residue, named | ι |
| gui-value-field-get-modules | same | `gui/src-tauri/src/engine/*.rs` | (after ε) | post-ι residue, named | ι |
| gui-finish-mapping-copy | `"Matte" =>` | `gui/src-tauri/src/engine/*.rs` | 2 (in engine.rs) | 1 | ι |
| gui-mechanism-map-key-root | `Value::String\("(kind\|bodies\|at\|world\|axis\|range)"` | `gui/src-tauri/src/engine.rs` | 8 | 0 | κ |
| gui-mechanism-map-key-modules | same | `gui/src-tauri/src/engine/*.rs` | (after δ) | 0 | κ |
| gui-session-event-setter | `fn set_[a-z_]+_(emitter\|sink)\(` | `gui/src-tauri/src/engine.rs` | 8 | 0 | λ |
| mcp-hand-parsed-params | `params\["` | `crates/reify-mcp/src/tools/*.rs` | 17 | 0 | ν |
| debug-nested-submit | `block_on\(` | `gui/src-tauri/src/debug_*.rs` | 0 | 0 | π |
| debug-engine-handle | `Arc<Mutex<EngineSession>>\|run_on_engine\(\|spawn_on_large_stack\(` | `gui/src-tauri/src/debug_*.rs` | 27 | 0 | ρ |
| debug-baseline-side-effect | `compute_delta\(\|last_state` | `gui/src-tauri/src/debug_*.rs` | 55 | 0 | ρ |
| debug-serial-ops-assumption | `serial-debug-ops\|never overlap\|run serially` | `gui/src-tauri/src/debug_*.rs` | 12 | 0 | ρ |
| debug-full-state-push | `apply_gui_state\|push_gui_state\(` | `gui/src-tauri/src/debug_*.rs` | 18 | 0 | ρ |
| debug-full-state-push-bridge | `apply_gui_state` | `gui/src/debug/bridge.ts` | 3 | 0 | ρ |
| debug-tool-name-regex-src | `readDebugServerSource\|extractDispatchArmNames\|extractToolDefNames` | `gui/src/__tests__/*.ts` | 26 | 0 | τ |
| debug-tool-name-regex-visual | `readDebugServerSource\|extractToolDefNames` | `gui/test/visual/*.ts` | 2 | 0 | τ |

(`|` inside an ERE is escaped as `\|` only for this Markdown table; the manifest row carries the
bare `|`. `gui/src-tauri/src/debug_*.rs` matches `debug_server.rs` today and `debug_protocol.rs` /
`debug_tools.rs` later; `debug.rs` is excluded by the glob.)

## Gap-fill amendments (2026-10-16)

Applied from Leo's 2026-10-16 gap-fill sitting (`plans/gap-fill-deliberation-reify-20261016.md`). The
capability-manifest pair carries the new delivered checks.

- **K7-D2** (`§K7-D2`): λ #8947 gains one backend-only `design_loaded(canonical_path)` SessionEvents event
  (no Tauri emit, so no bindings.ts change; sentence-sized under G1). Its consumer, `DesignWatch::reconcile`
  (re-arms the FS watcher only when the path differs; the explicit `watch_file` calls deleted), is a leaf of
  the new **gui-engine-admission** stream after λ and ρ #8954, not of this PRD.
- **K7-D1** (`§K7-D1`, Leo B): GUI admission policy moves to the new **gui-engine-admission** stream
  (`EngineSession::resolve_target` → `EntryDocument`, `persistable_source`, PreviewState, `WriteTarget::admit`,
  DesignWatch, `SidecarState::require_ready`), sequenced after ζ #8942, λ #8947 and ξ #8950. ξ #8950 keeps
  its active-file guard behind one named function, which that stream re-homes onto `resolve_target` and
  deletes. #8574 and #8471 are to be absorbed by that stream (notes on both; not cancelled now). This
  PRD's §4.10 placement row 3 stands until that stream lands.
- **K7-D10 (3)** (`§Uncontested block → K7-D10`): new G1 follow-on leaf **μ₂ #9278** after μ #8948 —
  `push_serialized_event` moves into `gui_state_schema.rs` and the reify-gui `[diff, gui_state_schema, types]`
  row leaves `scripts/module-scc-baseline.json` (fk-65db0c1c9ef3, low). Checks `macro-support-out-of-diff`,
  `macro-support-in-schema`.
- Cross-reference: K7-D3's standalone **#9279** (the chat sidecar's landlock workspace follows the open
  design; respawn on change) depends on λ #8947 for `design_loaded`.
- **2026-10-19, gui-engine-admission** (gap-fill lead, from that stream's cross-reference lines; `docs/prds/v0_6/gui-engine-admission.md`): §4.10 placement row 3 is superseded when that PRD's α #9387 lands, because the host guard is re-homed onto `EntryDocument::resolve_target`. ξ #8950, η #8943 and ο #8951 carry notes.
