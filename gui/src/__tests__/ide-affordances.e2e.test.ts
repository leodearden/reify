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
  renderEditorInHarness,
  makeDispatch,
  type DebugRequestHandler,
  type LspCall,
  type HarnessSetup,
} from './ideAffordancesHarness';
import { flushMacrotasks } from './test-utils';

// jsdom lacks document.elementFromPoint — stub it (mirrors debugBridge.test.tsx:46-53).
if (typeof document.elementFromPoint !== 'function') {
  Object.defineProperty(document, 'elementFromPoint', {
    configurable: true,
    writable: true,
    value: (): Element | null => null,
  });
}

// ── Test state ────────────────────────────────────────────────────────────────

let harness: HarnessSetup;
let dispatch: ReturnType<typeof makeDispatch>;

beforeEach(async () => {
  vi.clearAllMocks();
  // Re-apply base listen mock (setupBridgeHarness overrides this with its own impl).
  vi.mocked(listen).mockResolvedValue(() => {});
  vi.mocked(invoke).mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  if (harness?.dispose) harness.dispose();
  delete window.__REIFY_DEBUG__;
  document.body.innerHTML = '';
  harness = undefined as unknown as HarnessSetup;
});

/**
 * step-1: RED smoke — drives real bridge+stores+Editor end-to-end in jsdom.
 * step-2: GREEN — harness mount internals implemented in ideAffordancesHarness.tsx.
 *
 * step-3: RED find-references — asserts textDocument/references request AND
 *         FindUsesPanel rows. Fails at rows assertion because step-3 does NOT
 *         wire FindUsesPanel yet (renderEditorInHarness renders only Editor).
 * step-4: GREEN — adds renderEditorWithFindUsesPanel to the harness; test updated
 *         to use it and the rows assertion passes.
 */

// ── step-1: RED smoke ─────────────────────────────────────────────────────────
//
// Drives the REAL bridge + REAL stores + REAL Editor end-to-end in jsdom.
// GREEN is implemented in step-2 (harness mount internals).

describe('IDE affordances harness — smoke (step-1/step-2)', () => {
  it('open_file returns fixture content, editorView is live, keyboard round-trips', async () => {
    // step-2 GREEN: mount the real Editor so editorView is registered.
    harness = await setupBridgeHarness();
    dispatch = makeDispatch(harness.handler);

    // Mount the real Editor AFTER bridge init so onMount registers editorView.
    renderEditorInHarness(harness);

    // (a) open_file — stores the file in the editor store
    const openResult = await dispatch('open_file', { path: FIXTURE_PATH, content: FIXTURE }) as any;
    expect(openResult).toMatchObject({ ok: true, path: FIXTURE_PATH });

    // (b) window.__REIFY_DEBUG__ is set by initDebugBridge
    expect(window.__REIFY_DEBUG__).toBeDefined();

    // (c) editorView is registered by the mounted Editor's onMount
    expect(window.__REIFY_DEBUG__!.editorView).toBeDefined();

    // (d) editor_content — reads from the live EditorView buffer (ctx.editorView)
    const contentResult = await dispatch('editor_content', {}) as any;
    expect(contentResult.activeFile).toBe(FIXTURE_PATH);
    expect(contentResult.content).toBe(FIXTURE);

    // (e) focus_editor — calls ctx.editorView.focus()
    const focusResult = await dispatch('focus_editor', {}) as any;
    expect(focusResult).toMatchObject({ ok: true });

    // (f) keyboard — dispatches KeyboardEvent on document.activeElement; returns {ok:true}
    const kbResult = await dispatch('keyboard', { key: 'a' }) as any;
    expect(kbResult).toMatchObject({ ok: true });
  });
});

// ── step-3: RED find-references ───────────────────────────────────────────────
//
// Asserts two things:
//   (faces backend)  lsp_request recorder captured textDocument/references with
//                    the cursor position and includeDeclaration:true.
//   (faces frontend) FindUsesPanel is populated — 2 find-use-row elements.
//
// step-3 RED: renderEditorInHarness only (no FindUsesPanel) → rows.length fails.
// step-4 GREEN: import/use renderEditorWithFindUsesPanel from harness.

describe('find-references (step-3 RED → step-4 GREEN)', () => {
  it('Shift+F12 sends textDocument/references and populates FindUsesPanel with 2 rows', async () => {
    harness = await setupBridgeHarness();
    dispatch = makeDispatch(harness.handler);

    // step-3 RED: renderEditorInHarness renders only Editor (no FindUsesPanel).
    // step-4 GREEN: replace with renderEditorWithFindUsesPanel(harness).
    renderEditorInHarness(harness);

    // Open the two-structure fixture.
    await dispatch('open_file', { path: FIXTURE_PATH, content: FIXTURE });
    // Yield to let Solid reactive effects settle (Editor loads content into CM).
    await flushMacrotasks(0);

    // Position cursor on `width` at LSP line:1, char:8 (PartA.width declaration).
    // CM is 1-based: LSP line 1 → CM line 2. `  param ` = 8 chars before `width`.
    const view = window.__REIFY_DEBUG__!.editorView!;
    expect(view.state.doc.length).toBeGreaterThan(0); // ensure CM doc loaded
    const offset = view.state.doc.line(2).from + 8;
    view.dispatch({ selection: { anchor: offset } });

    // Dispatch Shift+F12 directly on CM contentDOM (mirrors Editor.test.tsx pattern).
    view.contentDOM.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F12', shiftKey: true, bubbles: true }),
    );

    // Flush async: lspClient.references() → invoke mock resolves → onResults fires.
    await flushMacrotasks(0);

    // (faces backend) textDocument/references was sent with correct position.
    const refsCall = harness.lspCalls.find((c) => c.method === 'textDocument/references');
    expect(refsCall).toBeDefined();
    const refsParams = refsCall!.params as {
      textDocument: { uri: string };
      position: { line: number; character: number };
      context: { includeDeclaration: boolean };
    };
    expect(refsParams.textDocument.uri).toBe(`file://${FIXTURE_PATH}`);
    expect(refsParams.position.line).toBe(1);
    expect(refsParams.position.character).toBe(8);
    expect(refsParams.context.includeDeclaration).toBe(true);

    // (faces frontend) FindUsesPanel shows 2 rows — PartA.width ×2 (decl + use).
    // step-3 RED: this assertion FAILS because FindUsesPanel is not rendered yet.
    const rows = document.querySelectorAll('[data-testid="find-use-row"]');
    expect(rows.length).toBe(2);
  });
});
