/**
 * IDE affordances end-to-end integration test (task 4211 λ).
 *
 * CI-able vitest/jsdom bridge test that drives the five IDE affordances
 * (rename, find-references, folding, command palette, hover-sync) through the
 * REAL reify-debug bridge against a real .ri buffer.
 *
 * Design decision: focused integration harness (real bridge + real stores +
 * real affordance components), NOT a full App mount. See plan.json design_decisions.
 *
 * step-1: RED smoke — harness mounts, open_file returns fixture content,
 *         editorView is live, keyboard round-trips {ok:true}.
 * step-2: GREEN — implement mount internals.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// ── Tauri mocks (hoisted before any imports) ──────────────────────────────────
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(),
  LogicalSize: class LogicalSize {
    constructor(w: number, h: number) {
      (this as unknown as { width: number }).width = w;
      (this as unknown as { height: number }).height = h;
    }
  },
}));
vi.mock('three', () => ({
  Box3: class {
    expandByObject() {}
    isEmpty() { return true; }
  },
  Vector3: class {},
}));
vi.mock('html-to-image', () => ({
  toPng: vi.fn().mockResolvedValue('data:image/png;base64,STUB'),
}));
// @tauri-apps/plugin-dialog is imported transitively (bridge.ts → src/bridge.ts);
// mock the save/open functions so module load succeeds without a Tauri context.
vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: vi.fn().mockResolvedValue(null),
  open: vi.fn().mockResolvedValue(null),
}));

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { cleanup } from '@solidjs/testing-library';
import {
  FIXTURE,
  FIXTURE_PATH,
  setupBridgeHarness,
  makeDispatch,
  type DebugRequestHandler,
  type LspCall,
} from './ideAffordancesHarness';

// jsdom lacks document.elementFromPoint — stub it (mirrors debugBridge.test.tsx:46-53).
if (typeof document.elementFromPoint !== 'function') {
  Object.defineProperty(document, 'elementFromPoint', {
    configurable: true,
    writable: true,
    value: (): Element | null => null,
  });
}

// ── Test state ────────────────────────────────────────────────────────────────

let handler: DebugRequestHandler;
let dispatch: ReturnType<typeof makeDispatch>;
let lspCalls: LspCall[];
let disposeRoot: () => void;

beforeEach(async () => {
  vi.clearAllMocks();
  // Re-apply listen mock each time (vi.clearAllMocks resets implementations).
  vi.mocked(listen).mockResolvedValue(() => {});
});

afterEach(() => {
  cleanup();
  if (disposeRoot) disposeRoot();
  delete window.__REIFY_DEBUG__;
  document.body.innerHTML = '';
  disposeRoot = undefined as unknown as () => void;
});

// ── step-1: RED smoke ─────────────────────────────────────────────────────────
//
// Drives the REAL bridge + REAL stores + REAL Editor end-to-end in jsdom.
// GREEN is implemented in step-2 (harness mount internals).

describe('IDE affordances harness — smoke (step-1)', () => {
  it('open_file returns fixture content, editorView is live, keyboard round-trips', async () => {
    // step-2 will implement mountHarness() which renders the real Editor and
    // wires it up so that:
    //   1. open_file sets the live EditorView buffer to FIXTURE
    //   2. editor_content reads back the fixture text
    //   3. window.__REIFY_DEBUG__.editorView is the mounted EditorView
    //   4. focus_editor + keyboard({key:'a'}) return {ok:true}
    //
    // Until then this test goes RED because there is no mounted EditorView.
    const harness = await setupBridgeHarness();
    handler = harness.handler;
    lspCalls = harness.lspCalls;
    disposeRoot = harness.dispose;
    dispatch = makeDispatch(handler);

    // (a) open_file — stores the file in the editor store
    const openResult = await dispatch('open_file', { path: FIXTURE_PATH, content: FIXTURE }) as any;
    expect(openResult).toMatchObject({ ok: true, path: FIXTURE_PATH });

    // (b) editor_content — requires ctx.editorView (registered by mounted Editor onMount)
    // Without a mounted Editor, this reads from the store snapshot (not the live buffer).
    const contentResult = await dispatch('editor_content', {}) as any;
    expect(contentResult.activeFile).toBe(FIXTURE_PATH);
    // Store snapshot content should match the fixture we opened.
    expect(contentResult.content).toBe(FIXTURE);

    // (c) window.__REIFY_DEBUG__ is set by initDebugBridge
    expect(window.__REIFY_DEBUG__).toBeDefined();
    // editorView is only set by the mounted Editor's onMount — RED until step-2.
    // This assertion will fail (undefined) until the real Editor is rendered.
    expect(window.__REIFY_DEBUG__!.editorView).toBeDefined();

    // (d) focus_editor — requires editorView to be registered
    const focusResult = await dispatch('focus_editor', {}) as any;
    expect(focusResult).toMatchObject({ ok: true });

    // (e) keyboard — dispatches on document.activeElement; should return {ok:true}
    const kbResult = await dispatch('keyboard', { key: 'a' }) as any;
    expect(kbResult).toMatchObject({ ok: true });
  });
});
