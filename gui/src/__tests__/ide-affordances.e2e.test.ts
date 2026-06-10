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
 * Each test drives a specific affordance through the real bridge and asserts
 * both the request sent to the backend and the effect applied in the app.
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
  FIXTURE_TREE,
  setupBridgeHarness,
  renderEditorInHarness,
  renderEditorWithFindUsesPanel,
  renderEditorWithPalette,
  renderEditorWithDesignTree,
  renderAllAffordances,
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

// ── smoke ─────────────────────────────────────────────────────────────────────
//
// Drives the REAL bridge + REAL stores + REAL Editor end-to-end in jsdom.

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

// ── rename ────────────────────────────────────────────────────────────────────
//
// Asserts:
//   (faces backend)  textDocument/prepareRename then textDocument/rename with the
//                    new name and correct 0-based position.
//   (faces frontend) editor buffer: PartA.width occurrences rewritten; PartB.width
//                    unchanged (frontend correctly applies the server-provided WorkspaceEdit).

describe('rename', () => {
  it('F2 renames PartA.width occurrences only, leaving PartB.width untouched', async () => {
    harness = await setupBridgeHarness();
    dispatch = makeDispatch(harness.handler);
    renderEditorWithFindUsesPanel(harness);

    await dispatch('open_file', { path: FIXTURE_PATH, content: FIXTURE });
    await flushMacrotasks(0);

    // Verify content before rename: both structures have `width`
    const view = window.__REIFY_DEBUG__!.editorView!;
    expect(view.state.doc.length).toBeGreaterThan(0);
    const before = view.state.doc.toString();
    expect(before).toContain('param width = 10.0'); // PartA.width decl
    expect(before).toContain('param width = 5.0');  // PartB.width decl

    // Position cursor on PartA.width declaration: LSP line:1, char:8 → CM line:2
    const offset = view.state.doc.line(2).from + 8;
    view.dispatch({ selection: { anchor: offset } });

    // Dispatch F2 on contentDOM to trigger renameCommand
    view.contentDOM.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F2', bubbles: true }),
    );

    // Flush async: prepareRename resolves → promptNewName called → input field
    await flushMacrotasks(0);

    // Find the inline rename field (data-testid='rename-field')
    const renameField = document.querySelector('[data-testid="rename-field"]') as HTMLInputElement | null;
    expect(renameField).not.toBeNull();
    expect(renameField!.value).toBe('width'); // pre-filled with placeholder

    // Type the new name and submit via Enter
    renameField!.value = 'newWidth';
    renameField!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    // Flush async: rename() resolves → applyWorkspaceEdit → CM doc updated
    await flushMacrotasks(0);

    // (faces backend) textDocument/prepareRename with correct position
    const prepareCall = harness.lspCalls.find((c) => c.method === 'textDocument/prepareRename');
    expect(prepareCall).toBeDefined();
    const prepareParams = prepareCall!.params as { textDocument: { uri: string }; position: { line: number; character: number } };
    expect(prepareParams.textDocument.uri).toBe(`file://${FIXTURE_PATH}`);
    expect(prepareParams.position.line).toBe(1);
    expect(prepareParams.position.character).toBe(8);

    // (faces backend) textDocument/rename with newWidth + correct position
    const renameCall = harness.lspCalls.find((c) => c.method === 'textDocument/rename');
    expect(renameCall).toBeDefined();
    const renameParams = renameCall!.params as { textDocument: { uri: string }; position: { line: number; character: number }; newName: string };
    expect(renameParams.textDocument.uri).toBe(`file://${FIXTURE_PATH}`);
    expect(renameParams.position.line).toBe(1);
    expect(renameParams.position.character).toBe(8);
    expect(renameParams.newName).toBe('newWidth');

    // (faces frontend) buffer: PartA.width → newWidth; PartB.width unchanged
    const after = view.state.doc.toString();
    expect(after).toContain('param newWidth = 10.0');  // PartA decl renamed
    expect(after).toContain('x_size = newWidth');       // PartA use renamed
    expect(after).toContain('param width = 5.0');       // PartB decl unchanged
    expect(after).toContain('radius = width / 2');      // PartB use unchanged
  });
});

// ── folding ───────────────────────────────────────────────────────────────────
//
// Asserts (pure frontend, no LSP):
//   - The Ctrl-Shift-[ binding is registered in the keymap facet.
//   - foldedRanges(view.state) has at least one range after invoking the binding.
//
// Note: synthetic keydown does not drive CodeMirror keymaps in jsdom; the test
// asserts the binding is registered and invokes run() directly on the EditorView
// — consistent with the Editor.test.tsx fallback pattern.

describe('folding', () => {
  it('Ctrl-Shift-[ binding is registered and folds at cursor when invoked', async () => {
    harness = await setupBridgeHarness();
    dispatch = makeDispatch(harness.handler);
    renderEditorInHarness(harness);

    await dispatch('open_file', { path: FIXTURE_PATH, content: FIXTURE });
    await flushMacrotasks(0);

    const view = window.__REIFY_DEBUG__!.editorView!;
    expect(view.state.doc.length).toBeGreaterThan(0);

    // Position cursor at start of `structure PartA {` (line 0 = CM line 1, offset 0)
    view.dispatch({ selection: { anchor: 0 } });

    // Find Ctrl-Shift-[ binding in keymap facet and invoke it directly.
    // (Synthetic keydown does not drive CodeMirror keymaps in jsdom.)
    const { foldedRanges } = await import('@codemirror/language');
    const { keymap } = await import('@codemirror/view');
    const allBindings = view.state.facet(keymap).flat();
    const foldBinding = allBindings.find((b) => b.key === 'Ctrl-Shift-[');
    expect(foldBinding).toBeDefined(); // confirms the binding is registered
    foldBinding!.run!(view);
    await flushMacrotasks(0);

    // Assert: at least one range is folded
    let foldCount = 0;
    foldedRanges(view.state).between(0, view.state.doc.length, () => { foldCount++; });
    expect(foldCount).toBeGreaterThan(0);
  });
});

// ── find-references ───────────────────────────────────────────────────────────
//
// Asserts:
//   (faces backend)  lsp_request recorder captured textDocument/references with
//                    the cursor position and includeDeclaration:true.
//   (faces frontend) FindUsesPanel is populated — 2 find-use-row elements.

describe('find-references', () => {
  it('Shift+F12 sends textDocument/references and populates FindUsesPanel with 2 rows', async () => {
    harness = await setupBridgeHarness();
    dispatch = makeDispatch(harness.handler);

    // Render Editor + FindUsesPanel with onShowReferences wiring.
    renderEditorWithFindUsesPanel(harness);

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
    const rows = document.querySelectorAll('[data-testid="find-use-row"]');
    expect(rows.length).toBe(2);
  });
});

// ── command palette ───────────────────────────────────────────────────────────
//
// Asserts:
//   (frontend) palette container is observable via data-testid 'command-palette'
//              after Ctrl+Shift+P (without focusing the editor — risk R2 bypass).
//   (frontend) at least one command row with data-testid 'command-palette-item'.
//   (frontend) clicking the first row runs the command and closes the palette.

describe('command palette', () => {
  it('Ctrl+Shift+P opens the palette; a command row is runnable', async () => {
    harness = await setupBridgeHarness();
    dispatch = makeDispatch(harness.handler);

    // Render Editor + CommandPalette with useKeyboardShortcuts(onCommandPalette) wired.
    const { lastCommandRan } = renderEditorWithPalette(harness);

    await dispatch('open_file', { path: FIXTURE_PATH, content: FIXTURE });
    await flushMacrotasks(0);

    // Do NOT call focus_editor — the global Ctrl+Shift+P handler is on document,
    // so it fires regardless of focus (risk R2 bypass via palette-global exemption).
    // Bridge keyboard dispatches on document.activeElement (body), bubbles to document.
    await dispatch('keyboard', { key: 'P', ctrl: true, shift: true });
    await flushMacrotasks(0);

    // (faces frontend) palette container must be observable via data-testid.
    const palette = document.querySelector('[data-testid="command-palette"]');
    expect(palette).not.toBeNull();

    // Palette should have at least one command row (data-testid='command-palette-item').
    const items = document.querySelectorAll('[data-testid="command-palette-item"]');
    expect(items.length).toBeGreaterThan(0);

    // Click the first command row to execute it.
    (items[0] as HTMLElement).click();
    await flushMacrotasks(0);

    // After running the command, the palette should close (onClose called).
    const paletteAfter = document.querySelector('[data-testid="command-palette"]');
    expect(paletteAfter).toBeNull();

    // The command id was recorded by the harness runCommand spy.
    expect(lastCommandRan()).toBe('reEvaluate');
  });
});

// ── hover-sync ────────────────────────────────────────────────────────────────
//
// Asserts:
//   (frontend) mouseenter on DesignTree tree-row-PartA sets
//              store_state().selection.hoveredEntity === 'PartA'.
//   (frontend) the row gains data-hovered='true' (driven by hoveredEntity prop).

describe('hover-sync', () => {
  it('mouseenter on tree-row-PartA sets hoveredEntity in store and marks row data-hovered', async () => {
    harness = await setupBridgeHarness();
    dispatch = makeDispatch(harness.handler);

    renderEditorWithDesignTree(harness);

    await dispatch('open_file', { path: FIXTURE_PATH, content: FIXTURE });
    await flushMacrotasks(0);

    // DesignTree renders FIXTURE_TREE rows unconditionally (tree prop is static).
    const partARow = document.querySelector('[data-testid="tree-row-PartA"]');
    expect(partARow).not.toBeNull();

    // Fire mouseenter — DesignTree.tsx:223 calls props.onHover?.(node.entity_path).
    partARow!.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }));
    await flushMacrotasks(0);

    // (faces frontend) store: hoveredEntity must be set to 'PartA'.
    const storeResult = await dispatch('store_state', {}) as any;
    expect(storeResult.selection.hoveredEntity).toBe('PartA');

    // (faces frontend) DOM: row should carry data-hovered='true'.
    expect(partARow!.getAttribute('data-hovered')).toBe('true');
  });
});

// ── console-error capture integration (step-15 / step-16) ────────────────────
//
// Proves the list_console_errors gate is NOT tautological.
//
// Step-15 RED: setupBridgeHarness() does not call installConsoleErrorCapture()
// so console.error stays unpatched, the module-global buffer stays empty,
// and list_console_errors returns count:0 — failing the expectations below.
//
// Step-16 GREEN: after installConsoleErrorCapture()+clearConsoleErrors() are
// wired into setupBridgeHarness(), the sentinel emission is captured and the
// combined-session gate's expect(count).toBe(0) becomes a real regression guard.

describe('list_console_errors — capture integration (step-15)', () => {
  it('captures console.error into the ring buffer — sentinel proves capture is live', async () => {
    harness = await setupBridgeHarness();
    dispatch = makeDispatch(harness.handler);

    // Emit sentinel errors AFTER bridge init and BEFORE dispatching the query.
    // For this assertion to hold, installConsoleErrorCapture() must have patched
    // console.error so the ring buffer receives these entries.
    console.error('SENTINEL-4211-gate', new Error('sentinel'));
    console.warn('SENTINEL-4211-warn');

    const result = await dispatch('list_console_errors', {}) as any;

    // Capture must be live: count must reflect the sentinel emission.
    expect(result.count).toBeGreaterThanOrEqual(1);
    // The sentinel message must appear in the captured errors array.
    const gateEntry = (result.errors as Array<{ message: string }>).find(
      (e) => e.message.includes('SENTINEL-4211-gate'),
    );
    expect(gateEntry).toBeDefined();
  });
});

// ── combined session ──────────────────────────────────────────────────────────
//
// Integration gate: one it() that exercises all five affordances in sequence
// through the real bridge:
//   1. rename    (F2 → PartA.width → newWidth)
//   2. find-refs (Shift+F12)
//   3. folding   (Ctrl+Shift+[, via keymap-facet fallback)
//   4. palette   (Ctrl+Shift+P → run command)
//   5. hover     (mouseenter on tree-row-PartA)
// Finally asserts list_console_errors count === 0.

describe('combined session — all five affordances', () => {
  it('rename → find-refs → folding → palette → hover all work in one open_file session', async () => {
    harness = await setupBridgeHarness();
    dispatch = makeDispatch(harness.handler);

    const { lastCommandRan } = renderAllAffordances(harness);

    await dispatch('open_file', { path: FIXTURE_PATH, content: FIXTURE });
    await flushMacrotasks(0);

    const view = window.__REIFY_DEBUG__!.editorView!;
    expect(view.state.doc.length).toBeGreaterThan(0);

    // 1. Rename: F2 on PartA.width → newWidth.
    const widthOffset = view.state.doc.line(2).from + 8;
    view.dispatch({ selection: { anchor: widthOffset } });
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'F2', bubbles: true }));
    await flushMacrotasks(0);
    const renameField = document.querySelector('[data-testid="rename-field"]') as HTMLInputElement | null;
    expect(renameField).not.toBeNull();
    renameField!.value = 'newWidth';
    renameField!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await flushMacrotasks(0);
    expect(view.state.doc.toString()).toContain('param newWidth = 10.0');
    expect(view.state.doc.toString()).toContain('param width = 5.0'); // PartB untouched

    // 2. Find-references: Shift+F12 at cursor (still on PartA line 2, char 8+).
    harness.lspCalls.length = 0; // clear so we can find this specific call
    view.dispatch({ selection: { anchor: widthOffset } });
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'F12', shiftKey: true, bubbles: true }));
    await flushMacrotasks(0);
    const refsCall = harness.lspCalls.find((c) => c.method === 'textDocument/references');
    expect(refsCall).toBeDefined();
    const refRows = document.querySelectorAll('[data-testid="find-use-row"]');
    expect(refRows.length).toBe(2);

    // 3. Folding: Ctrl+Shift+[ via keymap-facet fallback (risk R1 confirmed).
    view.dispatch({ selection: { anchor: 0 } });
    const { foldedRanges } = await import('@codemirror/language');
    const { keymap } = await import('@codemirror/view');
    const allBindings = view.state.facet(keymap).flat();
    const foldBinding = allBindings.find((b) => b.key === 'Ctrl-Shift-[');
    expect(foldBinding).toBeDefined();
    foldBinding!.run!(view);
    await flushMacrotasks(0);
    let foldCount = 0;
    foldedRanges(view.state).between(0, view.state.doc.length, () => { foldCount++; });
    expect(foldCount).toBeGreaterThan(0);

    // 4. Command palette: Ctrl+Shift+P.
    await dispatch('keyboard', { key: 'P', ctrl: true, shift: true });
    await flushMacrotasks(0);
    const palette = document.querySelector('[data-testid="command-palette"]');
    expect(palette).not.toBeNull();

    const paletteItems = document.querySelectorAll('[data-testid="command-palette-item"]');
    expect(paletteItems.length).toBeGreaterThan(0);
    (paletteItems[0] as HTMLElement).click();
    await flushMacrotasks(0);
    expect(document.querySelector('[data-testid="command-palette"]')).toBeNull();
    expect(lastCommandRan()).toBe('reEvaluate');

    // 5. Hover: mouseenter on tree-row-PartA.
    const partARow = document.querySelector('[data-testid="tree-row-PartA"]');
    expect(partARow).not.toBeNull();
    partARow!.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }));
    await flushMacrotasks(0);
    const storeResult = await dispatch('store_state', {}) as any;
    expect(storeResult.selection.hoveredEntity).toBe('PartA');

    // Integration gate: no console errors from the combined session.
    const errResult = await dispatch('list_console_errors', {}) as any;
    expect(errResult.count).toBe(0);
  });
});
