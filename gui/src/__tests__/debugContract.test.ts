/**
 * Debug contract boundary tests (task-4293, τ0):
 * Pin the coordinate/transport CONVENTION for the reify-debug MCP expansion.
 *
 * step-3: error-envelope + wiring characterization (bridge dispatch → error shapes)
 * step-5: coordinate-convention characterization (get_layout_metrics bounds frame)
 * step-7: pick↔raycast agreement (real three.js, no mock — pins screen→NDC→raycast)
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Mock Tauri APIs — bridge initialization requires listen + invoke.
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('html-to-image', () => ({
  toPng: vi.fn().mockResolvedValue('data:image/png;base64,STUB'),
}));

// NOTE: 'three' is intentionally NOT mocked here.
// Steps 3 and 5 use bridge handlers that don't invoke three (get_layout_metrics,
// get_window_state). Step 7 requires the REAL three.js Raycaster for raycast validation.

import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { initDebugBridge } from '../debug/bridge';
import type { DebugStores } from '../debug/types';

type DebugRequestHandler = (event: {
  payload: { id: number; command: string; params: Record<string, unknown> };
}) => Promise<void>;

function makeStores(): DebugStores {
  return {
    engine: {
      state: {
        meshes: {} as any,
        values: {} as any,
        constraints: {} as any,
        evalStatus: { phase: 'idle' },
      },
      initFromState: vi.fn(),
    },
    editor: {
      state: {
        openFiles: [],
        activeFile: null,
        dirtyFiles: [],
        externallyChanged: [],
        cursorPosition: null,
      },
      openFile: vi.fn(),
    },
    selection: {
      state: {
        selectedEntity: null,
        selectedEntities: [],
        anchorEntity: null,
        hoveredEntity: null,
        highlightedParams: [],
      } as any,
      selectEntity: vi.fn(),
      hoverEntity: vi.fn(),
    },
    claude: {
      state: {
        messages: [],
        sessionStatus: 'idle',
        currentMessageId: null,
      },
    },
    viewState: { resetToDefaultView: vi.fn() },
    layout: {
      state: {
        editorWidth: 300,
        sideWidth: 300,
        designTreeHeight: 160,
        propertyHeight: 200,
        constraintHeight: 140,
      },
    },
  };
}

/** Dispatch a command through the real debug bridge and return the parsed response. */
async function dispatchCmd(
  handler: DebugRequestHandler,
  id: number,
  command: string,
  params: Record<string, unknown>,
): Promise<unknown> {
  vi.mocked(invoke).mockClear();
  await handler({ payload: { id, command, params } });
  const calls = vi.mocked(invoke).mock.calls;
  const responseCall = calls.find((c) => c[0] === 'debug_response');
  expect(responseCall).toBeDefined();
  const payload = responseCall![1] as { id: number; result: string };
  return JSON.parse(payload.result);
}

// ─────────────────────────────────────────────────────────────────────────────
// step-3: Error-envelope + wiring characterization
//
// Pins: (a) unknown command → {error:"unknown command: <name>"}; (b) missing
// required param → {error:"selector is required"}; (c) invalid selector →
// {error:string}. These guard the in-band JSON {error:string} envelope that
// the Rust transport (step-1b) passes through verbatim, and the
// tool-def→dispatch→handler delegation through buildHandlers().
// ─────────────────────────────────────────────────────────────────────────────
describe('debug contract — error envelope + wiring (step-3)', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
    document.body.innerHTML = '';
  });

  it('(a) unknown command resolves to {error:"unknown command: <name>"}', async () => {
    await initDebugBridge(makeStores());
    expect(capturedHandler).toBeDefined();

    const result = (await dispatchCmd(capturedHandler!, 1, 'nonexistent_command_xyz', {})) as any;
    expect(result.error).toBe('unknown command: nonexistent_command_xyz');
  });

  it('(b) get_layout_metrics with no selector resolves to {error:"selector is required"}', async () => {
    await initDebugBridge(makeStores());

    const result = (await dispatchCmd(capturedHandler!, 2, 'get_layout_metrics', {})) as any;
    expect(result.error).toBe('selector is required');
  });

  it('(c) get_layout_metrics with a syntactically invalid selector resolves to {error:string}', async () => {
    await initDebugBridge(makeStores());

    const result = (await dispatchCmd(capturedHandler!, 3, 'get_layout_metrics', { selector: ':::' })) as any;
    // Must produce an error envelope, not a false-negative {exists:false}
    expect(typeof result.error).toBe('string');
    expect(result.exists).toBeUndefined();
  });

  it('envelope shape: error responses are plain objects with a string "error" field', async () => {
    // Regression guard: every dispatch arm must produce a parseable {error:string} envelope.
    await initDebugBridge(makeStores());

    const r1 = (await dispatchCmd(capturedHandler!, 4, 'another_unknown_cmd', {})) as any;
    expect(typeof r1).toBe('object');
    expect(typeof r1.error).toBe('string');

    const r2 = (await dispatchCmd(capturedHandler!, 5, 'get_layout_metrics', {})) as any;
    expect(typeof r2).toBe('object');
    expect(typeof r2.error).toBe('string');
  });
});
