/**
 * Cross-language parity guard (task-4348):
 * tool_defs() in debug_server.rs ↔ buildHandlers() in bridge.ts
 *
 * Invariant: every frontend-mediated tool_def has a handler (no runtime
 * "unknown command"), and every handler is either advertised in tool_defs()
 * or flagged as REST-only.  The two documented allowlists capture the
 * legitimate Rust↔TS asymmetries.
 */
import { describe, it, expect, vi } from 'vitest';

// Mirror debugContract.test.ts:11-20 — these three mocks make bridge.ts
// importable at collection time without a Tauri runtime.  They are proven
// sufficient: debugContract imports the same module with just these mocks.
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('html-to-image', () => ({
  toPng: vi.fn().mockResolvedValue('data:image/png;base64,STUB'),
}));

import { readFileSync } from 'node:fs';
import { join } from 'node:path';

// Namespace import — a missing export surfaces as a clean assertion
// (bridge.buildHandlers === undefined) rather than a module-load error.
import * as bridge from '../debug/bridge';

// --- source parsing ---

// Path: gui/src/__tests__ → ../../src-tauri/src/debug_server.rs
const RUST = readFileSync(
  join(__dirname, '../../src-tauri/src/debug_server.rs'),
  'utf-8',
);

// Captures 56 ToolDef literals.  \s spans the newline between `ToolDef {`
// and `name:`.  `struct ToolDef {`'s field `name: &'static str` is unquoted
// so it is NOT captured — only the string-literal name fields are.
const toolDefNames = [...RUST.matchAll(/ToolDef\s*\{\s*name:\s*"([a-z0-9_]+)"/g)].map(
  (m) => m[1],
);

// --- documented allowlists ---

/**
 * Tools with a named dispatch_tool arm resolved entirely in Rust.
 * None sends its OWN command to query_frontend, so no TS handler is needed:
 *   - health / engine_state / mesh_stats / morph_stats / mesh_morph_stats:
 *       fully resolved in the Rust dispatch_tool match arm.
 *   - load_fixture: handle_load_fixture resolves a fixture path then reuses
 *       the existing "open_file" frontend command — there is no "load_fixture"
 *       frontend command and therefore no load_fixture TS handler is needed.
 *  (open_file / wait_for / wait_for_idle / wait_for_selector also have named
 *   arms but DO call query_frontend internally → they keep TS handlers and
 *   are NOT pure-engine.)
 */
const PURE_ENGINE_SIDE = [
  'health',
  'engine_state',
  'mesh_stats',
  'morph_stats',
  'mesh_morph_stats',
  'load_fixture',
  // set_fea_case has a named dispatch_tool arm in Rust (handle_set_fea_case)
  // that calls session.set_active_fea_case() — no query_frontend call, so no
  // TS handler is needed (task 3026).
  'set_fea_case',
];

/**
 * Handlers reachable via the REST endpoint that are intentionally NOT
 * advertised in tools/list, so they have no tool_def in debug_server.rs.
 *
 * Also includes `apply_gui_state`: the landing point for the Rust
 * `query_frontend("apply_gui_state", ...)` push from `handle_set_fea_case`.
 * It is called FROM Rust TO the frontend (not invoked as an MCP tool by the
 * harness), so it has no tool_def entry (task 3026).
 */
const REST_ONLY_HANDLERS = ['clear_selection', 'toggle_select', 'apply_gui_state'];

// --- handler key set (hoisted; shared across checks c/d/e) ---
// Handler bodies are lazy arrows; buildHandlers construction only calls
// createLspClient() (a plain object literal) — a stub ctx is safe.
// Hoisted so (c), (d), (e) share a single construction; if the ctx contract
// ever changes there is one place to update the stub.
const handlers = Object.keys(bridge.buildHandlers({} as any));

// -----------------------------------------------------------------------

describe('debug MCP parity: tool_defs() ↔ buildHandlers()', () => {
  it('(a) buildHandlers is exported from bridge.ts', () => {
    // RED until step-2 adds `export` to the function declaration.
    expect(typeof bridge.buildHandlers).toBe('function');
  });

  it('(b) extraction sanity — exact count matches raw ToolDef literals, no duplicates', () => {
    // Cross-check against the raw `ToolDef {` literal count minus the struct
    // definition itself.  If a future tool name contains chars outside
    // [a-z0-9_] (e.g. uppercase or hyphen), the name regex silently drops it
    // while the raw count still includes it — surfacing drift rather than
    // masking it behind a >= floor.
    const rawToolDefCount = RUST.match(/ToolDef\s*\{/g)?.length ?? 0;
    expect(toolDefNames.length).toBe(rawToolDefCount - 1); // -1 for `struct ToolDef {` itself
    // No duplicate tool_def names in debug_server.rs.
    expect(new Set(toolDefNames).size).toBe(toolDefNames.length);
  });

  it('(c) every frontend-mediated tool_def has a handler (no runtime "unknown command")', () => {
    const missing = toolDefNames.filter(
      (n) => !PURE_ENGINE_SIDE.includes(n) && !handlers.includes(n),
    );
    expect(missing).toStrictEqual([]);
  });

  it('(d) every handler is advertised in tool_defs or flagged REST-only', () => {
    const extra = handlers.filter(
      (h) => !toolDefNames.includes(h) && !REST_ONLY_HANDLERS.includes(h),
    );
    expect(extra).toStrictEqual([]);
  });

  it('(e) allowlists are self-checking — each entry actually exhibits its asymmetry', () => {
    // PURE_ENGINE_SIDE: must be in tool_defs AND absent from handlers
    for (const name of PURE_ENGINE_SIDE) {
      expect(toolDefNames, `PURE_ENGINE_SIDE '${name}' must be in tool_defs()`).toContain(name);
      expect(handlers, `PURE_ENGINE_SIDE '${name}' must NOT have a TS handler`).not.toContain(
        name,
      );
    }
    // REST_ONLY_HANDLERS: must be in handlers AND absent from tool_defs
    for (const name of REST_ONLY_HANDLERS) {
      expect(handlers, `REST_ONLY '${name}' must have a TS handler`).toContain(name);
      expect(
        toolDefNames,
        `REST_ONLY '${name}' must NOT appear in tool_defs()`,
      ).not.toContain(name);
    }
  });
});
