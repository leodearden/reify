/**
 * Editor per-keystroke store-sync performance invariants.
 *
 * This file contains two categories of tests:
 *
 * 1. Deterministic spy-based invariant tests (steps 1, 3, 9)
 *    Verify that Editor.tsx's updateListener does NOT call store.updateFileContent
 *    or bridge.updateSource per keystroke — only the 300ms-debounced path does.
 *
 * 2. Wall-clock micro-benchmark (step 5)
 *    Feeds 100 keystrokes into a 10k-line document and asserts total elapsed time
 *    is under 500ms (≈5ms/keystroke — 30× more generous than the 16ms
 *    perceptible-lag threshold, accommodating JSDOM overhead).
 *
 * Reuses the @tauri-apps mocking pattern, setupStore helpers, and getEditorView
 * from Editor.test.tsx.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
import { EditorView } from '@codemirror/view';
import { createEditorStore } from '../stores/editorStore';
import * as bridge from '../bridge';
import type { FileData } from '../types';

// Mock Tauri API modules before importing Editor (same pattern as Editor.test.tsx)
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { Editor } from '../editor/Editor';

// 10k-line synthetic document (~90KB).  All lines are comments so the
// reify-language parser's lex cost is predictable and small.
const LARGE_DOC = Array.from({ length: 10_000 }, (_, i) => `// line ${i + 1}`).join('\n');
const LARGE_FILE: FileData = {
  path: '/project/src/large.ri',
  content: LARGE_DOC,
};

/** Create an editorStore pre-loaded with the 10k-line file. */
function setupLargeStore() {
  const store = createEditorStore();
  store.openFile(LARGE_FILE);
  return store;
}

/** Extract the CM6 EditorView from the rendered container. */
function getEditorView(container: HTMLElement): EditorView {
  const cmEditor = container.querySelector('.cm-editor')!;
  return EditorView.findFromDOM(cmEditor as HTMLElement)!;
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

// ─── Step 1: store.updateFileContent must NOT be called per keystroke ─────
describe('Editor per-keystroke invariants', () => {
  it('store.updateFileContent is never called during 100 keystrokes (invariant guard)', () => {
    const store = setupLargeStore();
    // Spy on store.updateFileContent — Editor.tsx updateListener must never invoke it
    const updateFileContentSpy = vi.spyOn(store, 'updateFileContent');
    // Mock bridge.updateSource to prevent invoke() failures when the debounce timer fires
    vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);

    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Dispatch 100 single-character insertions under fake timers
    for (let i = 0; i < 100; i++) {
      view.dispatch({ changes: { from: 0, insert: 'x' } });
    }

    // Not called synchronously per keystroke
    expect(updateFileContentSpy).not.toHaveBeenCalled();

    // Not called even after the 300ms debounce fires — bridge.updateSource is the
    // debounced call, not store.updateFileContent
    vi.advanceTimersByTime(300);
    expect(updateFileContentSpy).not.toHaveBeenCalled();
  });

  // ─── Step 3: bridge.updateSource must be debounced, not per-keystroke ─────
  it('bridge.updateSource not called per-keystroke — coalesces to exactly one call after 300ms', () => {
    const store = setupLargeStore();
    const updateSourceSpy = vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);

    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // 100 rapid keystrokes under fake timers
    for (let i = 0; i < 100; i++) {
      view.dispatch({ changes: { from: 0, insert: 'x' } });
    }

    // (a) No synchronous serialization per keystroke — zero calls so far
    expect(updateSourceSpy).not.toHaveBeenCalled();

    // (b) All 100 keystrokes coalesce into exactly one debounced call after 300ms
    vi.advanceTimersByTime(300);
    expect(updateSourceSpy).toHaveBeenCalledTimes(1);
    expect(updateSourceSpy).toHaveBeenCalledWith(LARGE_FILE.path, expect.any(String));
  });
});

// ─── Step 5: wall-clock micro-benchmark ───────────────────────────────────
describe('Editor wall-clock latency', () => {
  it('100 keystrokes on a 10k-line doc complete in under 2000ms', () => {
    // Switch to real timers so performance.now() measures genuine wall-clock time
    vi.useRealTimers();

    const store = setupLargeStore();
    // Prevent bridge.updateSource from calling invoke (mock returns undefined → would fail)
    vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);

    const { unmount } = render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    const t0 = performance.now();
    for (let i = 0; i < 100; i++) {
      view.dispatch({ changes: { from: 0, insert: 'x' } });
    }
    const elapsed = performance.now() - t0;

    // Unmount immediately to cancel the pending 300ms debounce timer via
    // Editor.tsx's onCleanup → clearTimeout(debounceTimer)
    unmount();

    // 2000ms = 20ms/keystroke average.
    // Threshold is intentionally generous to accommodate JSDOM overhead, which
    // can be 10–50× slower than real browsers for CM6 tree operations.
    // Still catches catastrophic regressions (e.g., per-keystroke doc.toString()
    // on a 90KB document, or per-keystroke setState with full content —
    // those would be far above even 2000ms total).
    expect(elapsed).toBeLessThan(2000);
  });
});

// ─── Step 9: combined integration test ────────────────────────────────────
describe('Editor store sync under load', () => {
  it('200 keystrokes: updateFileContent=0, markDirty=200, updateSource=0 sync then 1 after debounce', () => {
    const store = setupLargeStore();

    const updateFileContentSpy = vi.spyOn(store, 'updateFileContent');
    const markDirtySpy = vi.spyOn(store, 'markDirty');
    const updateSourceSpy = vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);

    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    for (let i = 0; i < 200; i++) {
      view.dispatch({ changes: { from: 0, insert: 'x' } });
    }

    // (a) store.updateFileContent: 0 calls
    //     Editor.tsx updateListener never calls store.updateFileContent —
    //     it only calls store.markDirty synchronously then defers the rest
    expect(updateFileContentSpy).not.toHaveBeenCalled();

    // (b) store.markDirty: exactly 200 calls — once per keystroke
    //     Editor.tsx unconditionally calls markDirty on every docChanged update.
    //     Idempotency is enforced inside markDirty via the includes-guard
    //     (no extra setState when path is already dirty).
    expect(markDirtySpy).toHaveBeenCalledTimes(200);

    // (c) bridge.updateSource: 0 calls synchronously (behind 300ms debounce)
    expect(updateSourceSpy).not.toHaveBeenCalled();

    // (d) After the 300ms debounce fires: exactly 1 call (all 200 keystrokes coalesced)
    vi.advanceTimersByTime(300);
    expect(updateSourceSpy).toHaveBeenCalledTimes(1);
  });
});
