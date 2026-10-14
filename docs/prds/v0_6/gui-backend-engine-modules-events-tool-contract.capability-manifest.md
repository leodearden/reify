# Capability manifest — gui-backend-engine-modules-events-tool-contract.md

Authored 2026-10-13/14 with the PRD (pass 1 of the `review-all-reify-20261010` program, stream
S25). Machine-readable twin: `gui-backend-engine-modules-events-tool-contract.capability-manifest.yaml`
(task ids stamped at `commit_planning`). Binds each leaf's signal to substrate evidence (G3 + G6
mechanized). No leaf is held.

**Code anchors** verified against main `b1a7be2982` (2026-10-13), unchanged on every S25 path at
`cf59570562` (2026-10-14). Cite-by-symbol; re-locate lines at implementation time.

**Probe provenance.** `target/debug/reify` built 2026-10-12. Committed fixture (new, unregistered —
η registers it in the rust-coupled fixture list in the diff whose Rust test reads it):

| fixture | tree-sitter `parse --quiet` (session cache) | `reify check` |
|---|---|---|
| `tests/prd-gate/fixtures/s25_case_switch_display_routing.ri` | exit 0, 0 ERROR/MISSING | exit 0, `All constraints satisfied.` (two `@optimized ... falling back to body-inlining` warnings: the CLI registers no multi-case trampoline; the GUI's `EngineSession` does) |

`grammar_confirmed = true` on every leaf: no novel syntax.

**Delivered-check polarity.** Every `expect: present` grep pattern below matched 0 times and every
`expect: absent` pattern matched ≥ 1 at `cf59570562` (measured with `git grep -E -c` over the listed
paths; counts in each binding), so each check changes verdict when its leaf lands. Paths that do
not exist yet (`engine/*.rs`, `debug_protocol.rs`, `debug_tools.rs`, `queued_engine.rs`) are named
where a dependency or the leaf itself creates them, so a move does not make a check vacuous. The
fixture row carries no delivered check: it lands with the PRD and would be green on arrival.

**Other tasks' checks this PRD flips** are corrected at decompose (PRD §8, OQ5); #8698's goes to
the lead.

**Adoption ratchets (INV candidate 5).** Rows in `tests/infra/adoption-ratchet.manifest` (engine:
S19 α #8678, landed), each added by its owner leaf at its post-extraction count; the EREs, today's
counts and ceilings are the PRD's Appendix A (one source; this manifest does not restate them).

## Per-leaf bindings

| leaf | capability | binding | verdict | delivered check |
|---|---|---|---|---|
| α | units-module | producer-self — unit list from the review record h14 entry for gui/src-tauri/src/engine.rs, corrected per PRD §2.2 (keys split out); subsumes #8595 | PASS | path `gui/src-tauri/src/engine/units.rs` present |
| α | value-input-cluster-moved | producer-self — 1 match in engine.rs at cf59570562 | PASS | grep `fn parse_value_string_for_cell` absent in `gui/src-tauri/src/engine.rs` |
| β | fea-channels-module | producer-self — 15 items, imports downward only (R1a); subsumes #8600 | PASS | path `gui/src-tauri/src/engine/fea_channels.rs` present |
| β | fea-channels-moved | producer-self — 1 match in engine.rs at cf59570562 | PASS | grep `fn apply_fea_channels` absent in `gui/src-tauri/src/engine.rs` |
| γ | display-routing-module | producer-self — with impl MergedTraitDefs and the inline display_style_extract_tests | PASS | path `gui/src-tauri/src/engine/display_routing.rs` present |
| γ | display-routing-moved | producer-self — 1 match in engine.rs at cf59570562 | PASS | grep `fn collect_display_routing` absent in `gui/src-tauri/src/engine.rs` |
| δ | mechanism-module | producer-self — scalar_to_f64 moves with it (pub(super)); #8698's check is widened to follow it (PRD §8, lead) | PASS | path `gui/src-tauri/src/engine/mechanism.rs` present |
| δ | mechanism-moved | producer-self — 1 match in engine.rs at cf59570562 | PASS | grep `fn extract_joints_from_mechanism` absent in `gui/src-tauri/src/engine.rs` |
| ε | projection-module | producer-self — unit 2 of the record plus the expression printer | PASS | path `gui/src-tauri/src/engine/projection.rs` present |
| ε | projection-moved | producer-self — 1 match in engine.rs at cf59570562 | PASS | grep `fn build_values` absent in `gui/src-tauri/src/engine.rs` |
| ζ | reload-present | producer-self — 0 matches at cf59570562; behaviour cover B2 (load_from_compiled resets like commit_state, RED first) | PASS | grep `fn reload\(` present in `gui/src-tauri/src/engine.rs` |
| ζ | compiled-source-present | producer-self — 0 matches at cf59570562 | PASS | grep `struct CompiledSource` present in `gui/src-tauri/src` |
| η | builder-present | producer-self — 0 matches at cf59570562; behaviour cover B1/B1' on tests/prd-gate/fixtures/s25_case_switch_display_routing.ri | PASS | grep `fn assemble_gui_state` present in `gui/src-tauri/src/engine.rs` |
| η | case-switch-fixture | tests/prd-gate/fixtures/s25_case_switch_display_routing.ri — tree-sitter parse --quiet exit 0, 0 ERROR/MISSING; reify check exit 0 "All constraints satisfied." (2026-10-13). Lands with the PRD, so it carries no delivered check (green on arrival). | PASS | — |
| θ | constraint-status-enum | producer-self — 0 matches at cf59570562; subsumes #7471 (Rust half) | PASS | grep `enum ConstraintStatus` present in `gui/src-tauri/src/types.rs` |
| θ | satisfaction-token-gone | producer-self — 1 match at cf59570562 | PASS | grep `fn satisfaction_token` absent in `gui/src-tauri/src` |
| θ | format-determinacy-gone | producer-self — 1 match at cf59570562 | PASS | grep `fn format_determinacy` absent in `gui/src-tauri/src/types.rs` |
| ι | result-views-module | producer-self — after β γ δ; ElasticResult breadcrumb to S15 ε #8690 | PASS | path `gui/src-tauri/src/engine/result_views.rs` present |
| κ | mechanism-map-keys-gone | producer-self — 3 matches in engine.rs at cf59570562 (get_mechanism_descriptors, is_world_sentinel, extract_joint_descriptor); δ moves two into engine/mechanism.rs, which the paths include; depends on S6 #8845, #8838, #8839 | PASS | grep `Value::String\("kind"` absent in `gui/src-tauri/src/engine.rs`, `gui/src-tauri/src/engine/mechanism.rs` |
| λ | session-events-present | producer-self — 0 matches at cf59570562 | PASS | grep `trait SessionEvents` present in `gui/src-tauri/src` |
| λ | fea-diagnostics-emitter-gone | producer-self — 1 match at cf59570562 | PASS | grep `trait FeaDiagnosticsEmitter` absent in `gui/src-tauri/src` |
| λ | deny-unused-must-use | producer-self — 0 matches at cf59570562 (critic C1-05: the attribute does not exist today) | PASS | grep `^#!\[deny\(unused_must_use\)\]` present in `gui/src-tauri/src/lib.rs` |
| μ | emitter-reason-gone | producer-self — 1 match (the full_reload_only reason) at cf59570562 | PASS | grep `fea-diagnostics-changed emitter` absent in `gui/src-tauri/src/types.rs` |
| μ | whole-opt-arm-present | producer-self — 0 matches at cf59570562 | PASS | grep `whole_opt` present in `gui/src-tauri/src/gui_state_schema.rs` |
| ν | tool-class-present | producer-self — 0 matches at cf59570562; subsumes #7722, #7071; depends #6665 | PASS | grep `^pub enum ToolClass` present in `crates/reify-mcp/src/registry.rs` |
| ν | hand-parsed-params-gone | producer-self — 17 matches at cf59570562 | PASS | grep `params\["` absent in `crates/reify-mcp/src/tools/read.rs`, `crates/reify-mcp/src/tools/write.rs`, `crates/reify-mcp/src/tools/navigation.rs`, `crates/reify-mcp/src/tools/reference.rs` |
| ν | update-result-gone | producer-self — 1 match at cf59570562 | PASS | grep `pub struct UpdateResult` absent in `crates/reify-mcp/src/types.rs` |
| ξ | produced-state-present | producer-self — 0 matches at cf59570562; subsumes #7181 | PASS | grep `fn take_produced_state` present in `gui/src-tauri/src/mcp_context.rs` |
| ξ | delegated-job-present | producer-self — 0 matches at cf59570562 | PASS | grep `fn delegated_tool_job` present in `gui/src-tauri/src/mcp_context.rs` |
| ο | debug-reimplementations-gone | producer-self — 22 matches in debug_server.rs at cf59570562 (debug_protocol.rs is created by #6964, a dependency); the delegation arm is named handle_delegated so it cannot match | PASS | grep `handle_reify_` absent in `gui/src-tauri/src/debug_server.rs`, `gui/src-tauri/src/debug_protocol.rs` |
| ο | mcp-tool-call-gone | producer-self — 1 match at cf59570562 | PASS | grep `fn mcp_tool_call\(` absent in `gui/src-tauri/src/main.rs` |
| ο | scanner-retired | producer-self — 1 match at cf59570562 | PASS | grep `^mod debug_write_tool_routing_tests;` absent in `gui/src-tauri/src/tests/mod.rs` |
| ο | stamped-present | producer-self — 0 matches at cf59570562 | PASS | grep `pub struct Stamped` present in `gui/src-tauri/src/eval_queue.rs` |
| σ | mcp-constraint-status-enum | producer-self — 0 matches at cf59570562 | PASS | grep `^pub enum ConstraintStatus` present in `crates/reify-mcp/src/types.rs` |
| σ | severity-typed | producer-self — 1 match at cf59570562 (DiagnosticInfo only; reify_core::Diagnostic is S12's) | PASS | grep `severity: String` absent in `crates/reify-core/src/diagnostics.rs` |
| σ | keyword-present | producer-self — 0 matches at cf59570562 | PASS | grep `pub fn keyword` present in `crates/reify-compiler/src/types.rs` |
| π | queued-engine-present | producer-self — 0 matches across gui/src-tauri/src at cf59570562; own module so no {eval_queue, engine_activity} SCC (critic C1-02) | PASS | grep `^pub struct QueuedEngine` present in `gui/src-tauri/src/queued_engine.rs` |
| π | frontend-port-present | producer-self — 0 matches at cf59570562 | PASS | grep `^pub trait FrontendPort` present in `gui/src-tauri/src/debug.rs` |
| ρ | run-on-engine-gone | producer-self — 12 matches in debug_server.rs at cf59570562; subsumes #7854 | PASS | grep `run_on_engine\(` absent in `gui/src-tauri/src/debug_server.rs`, `gui/src-tauri/src/debug_protocol.rs` |
| ρ | baseline-side-effect-gone | producer-self — 6 matches in debug_server.rs at cf59570562 | PASS | grep `compute_delta\(` absent in `gui/src-tauri/src/debug_server.rs`, `gui/src-tauri/src/debug_protocol.rs` |
| ρ | full-state-push-gone | producer-self — 3 matches at cf59570562 | PASS | grep `apply_gui_state` absent in `gui/src/debug/bridge.ts` |
| τ | tool-route-present | producer-self — 0 matches across gui/src-tauri/src at cf59570562 | PASS | grep `^pub enum ToolRoute` present in `gui/src-tauri/src/debug_tools.rs` |
| τ | export-committed | producer-self — regenerate-and-diff test BT7 | PASS | path `gui/src-tauri/debug-tools.json` present |
| τ | source-regex-reader-gone | producer-self — 1 match at cf59570562 | PASS | grep `readDebugServerSource` absent in `gui/src/__tests__/toolDefNames.ts` |
| τ | dispatch-arm-regex-gone | producer-self — 3 matches at cf59570562 | PASS | grep `extractDispatchArmNames` absent in `gui/src/__tests__/debugParity.test.ts` |
| ω | terminal-status | producer-self — the committed header (overlay "PRD terminal status"); no delivered check (a docs edit) | PASS | — |

No binding FAILs. No leaf asserts a numeric bound, so the G6 numeric-floor branch does not fire.

