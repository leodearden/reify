import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { deferred } from './test-utils';
import { render, screen } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { EditorView, keymap } from '@codemirror/view';
import { undo } from '@codemirror/commands';
import { foldAll, unfoldAll, foldedRanges } from '@codemirror/language';
import { diagnosticCount, forEachDiagnostic } from '@codemirror/lint';
import type { DiagnosticInfo } from '../types';
import { createEditorStore } from '../stores/editorStore';
import * as bridge from '../bridge';
import type { FileData, SourceLocation } from '../types';
import { EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG } from '../editor/messages';

// Mock Tauri API modules before importing Editor
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Editor, EDITOR_DEBOUNCE_MS } from '../editor/Editor';

const mockListen = vi.mocked(listen);
const mockInvoke = vi.mocked(invoke);

const file1: FileData = { path: '/project/src/bracket.ri', content: 'structure Bracket {\n  param width = 80mm\n}' };
const file2: FileData = { path: '/project/src/mount.ri', content: 'structure Mount {}' };
const file3: FileData = { path: '/project/src/plate.ri', content: 'structure Plate {}' };

beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

function setupStore(files: FileData[] = [file1]) {
  const store = createEditorStore();
  for (const f of files) store.openFile(f);
  return store;
}

/** Capture the Tauri diagnostics event handler so we can fire events manually. */
function setupListenCapture() {
  let diagnosticsHandler: ((event: { payload: any }) => void) | undefined;
  mockListen.mockImplementation(async (_event: any, handler: any) => {
    diagnosticsHandler = handler;
    return vi.fn(); // unlisten
  });
  return () => diagnosticsHandler;
}

describe('Editor mounting', () => {
  it('renders container div with data-testid', () => {
    const store = setupStore();
    render(() => <Editor store={store} />);
    expect(screen.getByTestId('editor-container')).toBeTruthy();
  });

  it('container has a .cm-editor child (CM6 mounted)', () => {
    const store = setupStore();
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const cmEditor = container.querySelector('.cm-editor');
    expect(cmEditor).not.toBeNull();
  });

  it('editor contains active file content', () => {
    const store = setupStore();
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const cmContent = container.querySelector('.cm-content');
    expect(cmContent).not.toBeNull();
    expect(cmContent!.textContent).toContain('structure Bracket');
  });
});

/** Get the CM6 EditorView instance from the rendered container. */
function getEditorView(container: HTMLElement): EditorView {
  const cmEditor = container.querySelector('.cm-editor')!;
  return EditorView.findFromDOM(cmEditor as HTMLElement)!;
}

describe('Editor doc change handling', () => {
  it('editing marks file as dirty immediately', () => {
    const store = setupStore();
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Insert text via CM6 dispatch
    view.dispatch({ changes: { from: 0, insert: '// comment\n' } });

    expect(store.state.dirtyFiles).toContain(file1.path);
  });

  it('calls bridge.updateSource at EDITOR_DEBOUNCE_MS boundary — not before, yes after', () => {
    const store = setupStore();
    const updateSpy = vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    view.dispatch({ changes: { from: 0, insert: '// comment\n' } });

    // Not called immediately
    expect(updateSpy).not.toHaveBeenCalled();

    // Must NOT fire 1ms before the boundary
    vi.advanceTimersByTime(EDITOR_DEBOUNCE_MS - 1);
    expect(updateSpy).not.toHaveBeenCalled();

    // Must fire exactly at the EDITOR_DEBOUNCE_MS boundary
    vi.advanceTimersByTime(1);
    expect(updateSpy).toHaveBeenCalledWith(file1.path, expect.stringContaining('// comment'));
  });

  it('rapid edits collapse into a single updateSource call', () => {
    const store = setupStore();
    const updateSpy = vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Three rapid edits
    view.dispatch({ changes: { from: 0, insert: 'a' } });
    vi.advanceTimersByTime(100);
    view.dispatch({ changes: { from: 1, insert: 'b' } });
    vi.advanceTimersByTime(100);
    view.dispatch({ changes: { from: 2, insert: 'c' } });

    // Not yet
    expect(updateSpy).not.toHaveBeenCalled();

    // 300ms after last edit
    vi.advanceTimersByTime(EDITOR_DEBOUNCE_MS);
    expect(updateSpy).toHaveBeenCalledTimes(1);
  });

  it('doc change with null activeFile does not call markDirty or updateSource', () => {
    // setupStore([]) leaves activeFile as null — validates the `if (path)` guard at Editor.tsx:139
    const store = setupStore([]);
    const markDirtySpy = vi.spyOn(store, 'markDirty');
    const updateSpy = vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Dispatch a doc change — the EditorView has an empty doc but accepts dispatches
    view.dispatch({ changes: { from: 0, insert: 'x' } });

    // markDirty must not be called immediately (no activeFile to mark)
    expect(markDirtySpy).not.toHaveBeenCalled();

    // Advance past the debounce timer — updateSource must also not be called
    vi.advanceTimersByTime(EDITOR_DEBOUNCE_MS);
    expect(updateSpy).not.toHaveBeenCalled();
  });

  it('doc change with activeFile transitioning to null before debounce fires does not call updateSource', () => {
    // Start with one open file so activeFile is set
    const store = setupStore([file1]);
    const updateSpy = vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Dispatch a doc change — starts the 300ms debounce timer
    view.dispatch({ changes: { from: 0, insert: 'x' } });

    // Mid-session: close the only file, setting activeFile back to null
    store.closeFile(file1.path);

    // When closeFile sets activeFile to null, the createEffect (Editor.tsx:218) fires and clears the debounce timer, preventing the updateSource call.
    vi.advanceTimersByTime(EDITOR_DEBOUNCE_MS);
    expect(updateSpy).not.toHaveBeenCalled();
  });
});

describe('Editor save (Ctrl+S)', () => {
  it('save calls bridge.saveFile with active file path', () => {
    const store = setupStore();
    store.markDirty(file1.path);
    const saveSpy = vi.spyOn(bridge, 'saveFile').mockResolvedValue(undefined);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Simulate Ctrl+S by dispatching the key through CM6
    // Use the CM6 keymap dispatch by pressing Ctrl-s
    const event = new KeyboardEvent('keydown', {
      key: 's',
      code: 'KeyS',
      ctrlKey: true,
      bubbles: true,
    });
    view.contentDOM.dispatchEvent(event);

    expect(saveSpy).toHaveBeenCalledWith(file1.path, file1.content);
  });

  it('save aborts without calling saveFile when file is not in store', () => {
    const store = setupStore([file1]);
    // Set activeFile to a path that is NOT in openFiles
    store.setActiveFile('/project/src/missing.ri');
    const saveSpy = vi.spyOn(bridge, 'saveFile').mockResolvedValue(undefined);
    const cleanSpy = vi.spyOn(store, 'markClean');
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Simulate Ctrl+S
    const event = new KeyboardEvent('keydown', {
      key: 's',
      code: 'KeyS',
      ctrlKey: true,
      bubbles: true,
    });
    view.contentDOM.dispatchEvent(event);

    // saveFile should NOT be called — the file isn't in the store
    expect(saveSpy).not.toHaveBeenCalled();
    // markClean should also NOT be called
    expect(cleanSpy).not.toHaveBeenCalled();
  });

  it('save clears dirty flag', async () => {
    const store = setupStore();
    store.markDirty(file1.path);
    vi.spyOn(bridge, 'saveFile').mockResolvedValue(undefined);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    expect(store.state.dirtyFiles).toContain(file1.path);

    const event = new KeyboardEvent('keydown', {
      key: 's',
      code: 'KeyS',
      ctrlKey: true,
      bubbles: true,
    });
    view.contentDOM.dispatchEvent(event);

    // markClean now runs after the saveFile promise resolves
    await vi.waitFor(() => {
      expect(store.state.dirtyFiles).not.toContain(file1.path);
    });
  });

  it('Ctrl+S saves the live buffer content, not the stale store snapshot', () => {
    // Uses the anti-loop invariant: typing changes the view doc but NOT the store snapshot.
    // Ctrl+S must read the live CodeMirror doc, not the stale store content.
    const fileA: FileData = { path: '/a.ri', content: 'INITIAL' };
    const store = setupStore([fileA]);
    const saveSpy = vi.spyOn(bridge, 'saveFile').mockResolvedValue(undefined);
    vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Type 'X' at position 0 — view.doc becomes 'XINITIAL'
    // but the store snapshot stays 'INITIAL' (anti-loop invariant: updateListener
    // calls markDirty + debounced updateSource, never updateFileContent).
    view.dispatch({ changes: { from: 0, insert: 'X' } });

    // DO NOT advance fake timers — debounced updateSource must not fire.
    // Confirm the divergence: live doc differs from store snapshot.
    expect(view.state.doc.toString()).toBe('XINITIAL');
    const storeFile = store.state.openFiles.find((f) => f.path === fileA.path);
    expect(storeFile!.content).toBe('INITIAL'); // store unchanged

    // Dispatch Ctrl+S on the CM contentDOM
    const event = new KeyboardEvent('keydown', {
      key: 's',
      code: 'KeyS',
      ctrlKey: true,
      bubbles: true,
    });
    view.contentDOM.dispatchEvent(event);

    // Assert live buffer content was saved, NOT the stale store snapshot
    expect(saveSpy).toHaveBeenCalledWith(fileA.path, 'XINITIAL');
  });
});

describe('Editor liveContentRef prop', () => {
  it('liveContentRef receives a getter that returns the live CM view document', () => {
    // When App passes liveContentRef, Editor should call it in onMount with a
    // getter () => view.state.doc.toString() | null so App can read the live
    // buffer at save/re-evaluate time without per-keystroke store writes.
    const fileA: FileData = { path: '/a.ri', content: 'INITIAL' };
    const store = setupStore([fileA]);
    vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);

    let captured: (() => string | null) | undefined;
    render(() => (
      <Editor
        store={store}
        liveContentRef={(getter) => {
          captured = getter;
        }}
      />
    ));
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // After mount, the getter must be wired and return the initial doc
    expect(captured).toBeDefined();
    expect(captured!()).toBe('INITIAL');

    // Type 'X' at position 0 — view.doc becomes 'XINITIAL', store stays 'INITIAL'
    view.dispatch({ changes: { from: 0, insert: 'X' } });

    // Getter must reflect the live doc, not the stale store snapshot
    expect(captured!()).toBe('XINITIAL');
    const storeFile = store.state.openFiles.find((f) => f.path === fileA.path);
    expect(storeFile!.content).toBe('INITIAL'); // confirm store unchanged
  });
});

describe('Editor cursor tracking', () => {
  it('dispatching selection update sets cursor position in store', () => {
    const store = setupStore();
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Move cursor to offset 25 within the editor document.
    // file1 content: 'structure Bracket {\n  param width = 80mm\n}'
    //  line 1: 'structure Bracket {' (19 chars) + '\n' → line 2 starts at offset 20
    //  offset 25 = pos - line.from + 1 = (25 - 20) + 1 = 6 (1-based column)
    //  The 6th character of line 2 is 'a' in '  param width = 80mm'.
    const offset = 25;
    view.dispatch({ selection: { anchor: offset } });

    expect(store.state.cursorPosition).not.toBeNull();
    expect(store.state.cursorPosition!.line).toBe(2);
    // 1-based column to match the backend convention used by
    // getEntityAtSourceLocation / getContainingDefinition.
    expect(store.state.cursorPosition!.column).toBe(6);
  });
});

describe('Editor active file switching', () => {
  it('changing activeFile updates editor document content', () => {
    // Open both files, explicitly set activeFile to file1
    const store = setupStore([file1, file2]);
    store.setActiveFile(file1.path);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Initially shows file1 content
    expect(view.state.doc.toString()).toContain('structure Bracket');

    // Switch to file2
    store.setActiveFile(file2.path);

    // Editor should now show file2 content
    const updatedView = getEditorView(container);
    expect(updatedView.state.doc.toString()).toContain('structure Mount');
    expect(updatedView.state.doc.toString()).not.toContain('structure Bracket');
  });

  it('switching back restores original content', () => {
    // Open both files, explicitly set activeFile to file1
    const store = setupStore([file1, file2]);
    store.setActiveFile(file1.path);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');

    // Initially shows file1 content
    let view = getEditorView(container);
    expect(view.state.doc.toString()).toContain('structure Bracket');

    // Switch to file2
    store.setActiveFile(file2.path);
    view = getEditorView(container);
    expect(view.state.doc.toString()).toContain('structure Mount');

    // Switch back to file1
    store.setActiveFile(file1.path);
    view = getEditorView(container);
    expect(view.state.doc.toString()).toContain('structure Bracket');
    expect(view.state.doc.toString()).not.toContain('structure Mount');
  });
});

describe('Editor scrollToLocation', () => {
  const BASELINE_HEAD = 9; // file2 'structure Mount {}': end_column 10 -> 0-indexed offset 9

  function setupScrollToWithFile2Active() {
    const store = setupStore([file1, file2]);
    store.setActiveFile(file2.path);
    const [scrollTo, setScrollTo] = createSignal<SourceLocation | null>(null);
    render(() => <Editor store={store} scrollToLocation={scrollTo} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);
    return { view, setScrollTo };
  }

  it('setting scrollToLocation signal moves cursor to target line/column', () => {
    const store = setupStore();
    const [scrollTo, setScrollTo] = createSignal<SourceLocation | null>(null);
    render(() => <Editor store={store} scrollToLocation={scrollTo} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // file1 content: 'structure Bracket {\n  param width = 80mm\n}'
    // line 2, column 3 -> offset = 20 (line 1 + \n) + 2 (0-based col 3 -> index 2) = 22
    // line 2, column 8 -> offset = 20 + 7 = 27
    const location: SourceLocation = {
      file_path: file1.path,
      line: 2,
      column: 3,
      end_line: 2,
      end_column: 8,
    };

    setScrollTo(location);

    // After the effect runs, cursor should be at line 2
    const sel = view.state.selection.main;
    // anchor should be at start of span, head at end
    const line2 = view.state.doc.line(2);
    const expectedAnchor = line2.from + 2; // column 3, 0-indexed = 2
    const expectedHead = line2.from + 7; // column 8, 0-indexed = 7
    expect(sel.anchor).toBe(expectedAnchor);
    expect(sel.head).toBe(expectedHead);
  });

  it('scrollToLocation null is a no-op', () => {
    const store = setupStore();
    const [scrollTo] = createSignal<SourceLocation | null>(null);
    render(() => <Editor store={store} scrollToLocation={scrollTo} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Cursor should be at position 0 (default)
    expect(view.state.selection.main.head).toBe(0);
  });

  it('scrollToLocation targets active file with deterministic selection', () => {
    const { view, setScrollTo } = setupScrollToWithFile2Active();

    // file2 content: 'structure Mount {}' (single line, starts at offset 0)
    // end_column 10 -> head = 9 (0-indexed), column 5 -> anchor = 4 (0-indexed)
    setScrollTo({ file_path: file2.path, line: 1, column: 5, end_line: 1, end_column: 10 });
    expect(view.state.selection.main.anchor).toBe(4);
    expect(view.state.selection.main.head).toBe(BASELINE_HEAD);
  });

  it('scrollToLocation with mismatched file does not move cursor', () => {
    const { view, setScrollTo } = setupScrollToWithFile2Active();

    // Establish baseline: target active file2 and confirm cursor moved to head=BASELINE_HEAD
    setScrollTo({ file_path: file2.path, line: 1, column: 5, end_line: 1, end_column: 10 });
    expect(view.state.selection.main.head).toBe(BASELINE_HEAD);

    // Now target mismatched file1 — effect must not move cursor
    setScrollTo({ file_path: file1.path, line: 1, column: 5, end_line: 1, end_column: 10 });
    expect(view.state.selection.main.head).toBe(BASELINE_HEAD);
  });

  it('scrollToLocation with no active file does not crash', () => {
    // setupStore([]) leaves activeFile as null — tests the null guard
    const store = setupStore([]);
    const [scrollTo, setScrollTo] = createSignal<SourceLocation | null>(null);
    render(() => <Editor store={store} scrollToLocation={scrollTo} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Targeting any file when activeFile is null: string !== null, effect returns early
    setScrollTo({ file_path: file1.path, line: 1, column: 1, end_line: 1, end_column: 5 });

    // Cursor unmoved, no crash
    expect(view.state.selection.main.head).toBe(0);
  });

  it('scrollToLocation with file://-prefixed path matching active file succeeds', () => {
    // file1 is active; location uses the file:// URI form of the same path
    const store = setupStore();
    const [scrollTo, setScrollTo] = createSignal<SourceLocation | null>(null);
    render(() => <Editor store={store} scrollToLocation={scrollTo} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // file1.path = '/project/src/bracket.ri'
    // file:// URI form  = 'file:///project/src/bracket.ri'
    const location: SourceLocation = {
      file_path: 'file:///project/src/bracket.ri',
      line: 2,
      column: 3,
      end_line: 2,
      end_column: 8,
    };

    setScrollTo(location);

    // Cursor should have moved to line 2 (same as the bare-path test)
    const sel = view.state.selection.main;
    const line2 = view.state.doc.line(2);
    const expectedAnchor = line2.from + 2; // column 3, 0-indexed = 2
    const expectedHead = line2.from + 7;   // column 8, 0-indexed = 7
    expect(sel.anchor).toBe(expectedAnchor);
    expect(sel.head).toBe(expectedHead);
  });
});

describe('Editor save error callback', () => {
  it('calls onError when saveFile rejects', async () => {
    const store = setupStore();
    store.markDirty(file1.path);
    const onError = vi.fn();
    vi.spyOn(bridge, 'saveFile').mockRejectedValue(new Error('disk full'));
    render(() => <Editor store={store} onError={onError} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Simulate Ctrl+S
    const event = new KeyboardEvent('keydown', {
      key: 's',
      code: 'KeyS',
      ctrlKey: true,
      bubbles: true,
    });
    view.contentDOM.dispatchEvent(event);

    // Wait for the rejected promise to settle
    await vi.waitFor(() => {
      expect(onError).toHaveBeenCalledWith(expect.stringContaining('Failed to save file'));
        expect(onError).toHaveBeenCalledWith(expect.stringContaining('disk full'));
    });
  });
});

describe('Editor LSP init error callback', () => {
  it('calls onError when lspClient.initialize rejects', async () => {
    const store = setupStore();
    const onError = vi.fn();

    // Mock lspRequest to reject (lspClient.initialize calls bridge.lspRequest)
    vi.spyOn(bridge, 'lspRequest').mockRejectedValue(new Error('LSP unavailable'));

    render(() => <Editor store={store} onError={onError} />);

    // Wait for LSP init chain to settle (runs in onMount)
    await vi.waitFor(() => {
      expect(onError).toHaveBeenCalledWith(
        expect.stringContaining('LSP initialization failed'),
      );
    });
  });
});

describe('Editor diagnostics URI filtering', () => {
  const sampleDiagnostic = {
    range: {
      start: { line: 0, character: 0 },
      end: { line: 0, character: 9 },
    },
    severity: 1,
    message: 'test error',
  };

  it('diagnostics from non-active URI are NOT applied to editor', () => {
    const getHandler = setupListenCapture();
    const store = setupStore([file1]);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Verify handler was captured
    const handler = getHandler();
    expect(handler).toBeDefined();

    // Fire diagnostics for a DIFFERENT URI than the active file
    handler!({
      payload: {
        uri: 'file:///project/src/other.ri',
        diagnostics: [sampleDiagnostic],
      },
    });

    // Diagnostics should NOT have been applied to the editor
    expect(diagnosticCount(view.state)).toBe(0);
  });

  it('diagnostics from active file URI ARE applied to editor', () => {
    const getHandler = setupListenCapture();
    const store = setupStore([file1]);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    const handler = getHandler();
    expect(handler).toBeDefined();

    // Fire diagnostics for the ACTIVE file's URI
    handler!({
      payload: {
        uri: 'file:///project/src/bracket.ri',
        diagnostics: [sampleDiagnostic],
      },
    });

    // Diagnostics SHOULD have been applied
    expect(diagnosticCount(view.state)).toBe(1);
  });
});

describe('Editor per-file undo history (E-04)', () => {
  it('undo in file1 after switch round-trip restores file1 pre-edit content', () => {
    const store = setupStore([file1, file2]);
    store.setActiveFile(file1.path);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    let view = getEditorView(container);

    // (1) file1 is shown
    expect(view.state.doc.toString()).toContain('structure Bracket');

    // (2) Edit file1: insert '// edit\n' at position 0
    view.dispatch({ changes: { from: 0, insert: '// edit\n' } });
    expect(view.state.doc.toString()).toContain('// edit');

    // (3) Switch to file2
    store.setActiveFile(file2.path);
    view = getEditorView(container);
    expect(view.state.doc.toString()).toContain('structure Mount');

    // (4) Switch back to file1
    store.setActiveFile(file1.path);
    view = getEditorView(container);
    // file1 should still contain the edit
    expect(view.state.doc.toString()).toContain('// edit');

    // (5) Undo — should revert file1's edit, not produce file2 content
    undo(view);

    view = getEditorView(container);
    // After undo, file1 should be back to original content
    expect(view.state.doc.toString()).toBe(file1.content);
    // And NOT contain file2's content (undo history must not be polluted)
    expect(view.state.doc.toString()).not.toContain('structure Mount');
  });

  it('undo in file2 does NOT produce file1 content', () => {
    const store = setupStore([file1, file2]);
    store.setActiveFile(file1.path);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');

    // Switch to file2
    store.setActiveFile(file2.path);
    let view = getEditorView(container);

    // Attempt undo in file2 — should be a no-op (no edits made in file2)
    undo(view);

    view = getEditorView(container);
    // file2 should still show file2 content, NOT file1's content
    expect(view.state.doc.toString()).toContain('structure Mount');
    expect(view.state.doc.toString()).not.toContain('structure Bracket');
  });
});

describe('Editor debounce timer cancellation on file switch (RC-04)', () => {
  it('debounced updateSource does NOT fire after file switch', () => {
    const store = setupStore([file1, file2]);
    store.setActiveFile(file1.path);
    const updateSpy = vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Edit file1 (triggers debounce timer)
    view.dispatch({ changes: { from: 0, insert: '// edit\n' } });

    // Immediately switch to file2 (before 300ms elapses)
    store.setActiveFile(file2.path);

    // Advance timers past the debounce period
    vi.advanceTimersByTime(EDITOR_DEBOUNCE_MS);

    // The debounced updateSource should NOT have fired for the stale edit
    expect(updateSpy).not.toHaveBeenCalled();
  });
});

describe('Editor LSP file switch serialization (RC-02)', () => {
  it('rapid file switches serialize didClose/didOpen in correct order', async () => {
    const store = setupStore([file1, file2, file3]);
    store.setActiveFile(file1.path);

    // Track LSP method calls via the invoke mock
    const lspCalls: { method: string; uri?: string }[] = [];
    mockInvoke.mockImplementation(async (_cmd: string, args: any) => {
      const method = (args as any)?.method as string;
      if (method) {
        const params = JSON.parse((args as any)?.params ?? '{}');
        const uri = params?.textDocument?.uri;
        lspCalls.push({ method, uri });
      }
      // Return valid JSON for initialize (needs to be parsed)
      if ((args as any)?.method === 'initialize') {
        return JSON.stringify({ capabilities: {} });
      }
      return undefined as any;
    });

    render(() => <Editor store={store} />);

    // Wait for initial LSP calls (initialize, initialized, didOpen for file1)
    await vi.waitFor(() => {
      expect(lspCalls.some(c => c.method === 'textDocument/didOpen')).toBe(true);
    });
    lspCalls.length = 0;

    // Rapidly switch file1 -> file2 -> file3
    store.setActiveFile(file2.path);
    store.setActiveFile(file3.path);

    // Flush all microtasks — wait for all 4 LSP file-switch calls
    await vi.waitFor(() => {
      const switchCalls = lspCalls.filter(c => c.method.startsWith('textDocument/did'));
      expect(switchCalls).toHaveLength(4);
    });

    const fileSwitchCalls = lspCalls
      .filter(c => c.method.startsWith('textDocument/did'))
      .map(c => ({ method: c.method, uri: c.uri }));

    // Expected serialized order:
    // didClose(file1) -> didOpen(file2) -> didClose(file2) -> didOpen(file3)
    expect(fileSwitchCalls).toEqual([
      { method: 'textDocument/didClose', uri: 'file:///project/src/bracket.ri' },
      { method: 'textDocument/didOpen',  uri: 'file:///project/src/mount.ri' },
      { method: 'textDocument/didClose', uri: 'file:///project/src/mount.ri' },
      { method: 'textDocument/didOpen',  uri: 'file:///project/src/plate.ri' },
    ]);
  });
});

describe('Editor integration: rapid file switch with diagnostics mid-switch', () => {
  it('diagnostics, debounce, and LSP serialization all behave correctly on rapid switch', async () => {
    // Setup: capture diagnostics handler
    let diagnosticsHandler: ((event: { payload: any }) => void) | undefined;
    mockListen.mockImplementation(async (_event: any, handler: any) => {
      diagnosticsHandler = handler;
      return vi.fn();
    });

    // Track LSP calls
    const lspCalls: { method: string; uri?: string }[] = [];
    mockInvoke.mockImplementation(async (_cmd: string, args: any) => {
      const method = (args as any)?.method as string;
      if (method) {
        const params = JSON.parse((args as any)?.params ?? '{}');
        const uri = params?.textDocument?.uri;
        lspCalls.push({ method, uri });
      }
      if ((args as any)?.method === 'initialize') {
        return JSON.stringify({ capabilities: {} });
      }
      return undefined as any;
    });

    const store = setupStore([file1, file2]);
    store.setActiveFile(file1.path);
    const updateSpy = vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);

    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');

    // Wait for initial LSP setup
    await vi.waitFor(() => {
      expect(lspCalls.some(c => c.method === 'textDocument/didOpen')).toBe(true);
    });
    lspCalls.length = 0;

    // (1) Edit file A — triggers debounce timer
    const view = getEditorView(container);
    view.dispatch({ changes: { from: 0, insert: '// edit\n' } });

    // (2) Switch A -> B before debounce fires
    store.setActiveFile(file2.path);

    // (3) Fire diagnostics for file A's URI mid-switch
    expect(diagnosticsHandler).toBeDefined();
    diagnosticsHandler!({
      payload: {
        uri: 'file:///project/src/bracket.ri',
        diagnostics: [{
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } },
          severity: 1,
          message: 'stale diagnostic from file A',
        }],
      },
    });

    // (4) Advance timers past debounce period
    vi.advanceTimersByTime(EDITOR_DEBOUNCE_MS);

    // Verify: diagnostics NOT applied to B's editor (E-05 fix)
    const viewB = getEditorView(container);
    expect(diagnosticCount(viewB.state)).toBe(0);

    // Verify: debounced updateSource NOT called (RC-04 fix)
    expect(updateSpy).not.toHaveBeenCalled();

    // Verify: undo in file B does NOT produce file A content (E-04 fix)
    undo(viewB);
    expect(getEditorView(container).state.doc.toString()).toContain('structure Mount');
    expect(getEditorView(container).state.doc.toString()).not.toContain('structure Bracket');

    // Verify: LSP calls correctly ordered (RC-02 fix)
    await vi.waitFor(() => {
      const switchCalls = lspCalls.filter(c => c.method.startsWith('textDocument/did'));
      expect(switchCalls).toHaveLength(2);
    });

    const fileSwitchCalls = lspCalls
      .filter(c => c.method.startsWith('textDocument/did'))
      .map(c => ({ method: c.method, uri: c.uri }));

    expect(fileSwitchCalls).toEqual([
      { method: 'textDocument/didClose', uri: 'file:///project/src/bracket.ri' },
      { method: 'textDocument/didOpen',  uri: 'file:///project/src/mount.ri' },
    ]);
  });
});

describe('Editor cross-file goto-definition (E-12)', () => {
  it('Ctrl+Click on cross-file definition navigates via bridge.openFile and store.openFile', async () => {
    const store = setupStore([file1]);
    store.setActiveFile(file1.path);

    // Mock invoke to:
    // 1. Handle LSP init normally
    // 2. Return a cross-file definition location for textDocument/definition
    const crossFileLocation = {
      uri: 'file:///project/src/mount.ri',
      range: { start: { line: 5, character: 2 }, end: { line: 5, character: 10 } },
    };

    mockInvoke.mockImplementation(async (_cmd: string, args: any) => {
      const method = (args as any)?.method as string;
      if (method === 'initialize') {
        return JSON.stringify({ capabilities: {} });
      }
      if (method === 'textDocument/definition') {
        return JSON.stringify(crossFileLocation);
      }
      return undefined as any;
    });

    // Spy on bridge.openFile to return file2's data when called
    const openFileSpy = vi.spyOn(bridge, 'openFile').mockResolvedValue(file2);

    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // JSDOM has no layout, so posAtCoords returns null. Mock it to return a valid position.
    vi.spyOn(view, 'posAtCoords').mockReturnValue(5);

    // Simulate Ctrl+Click on contentDOM
    const mouseEvent = new MouseEvent('mousedown', {
      ctrlKey: true,
      clientX: 100,
      clientY: 50,
      bubbles: true,
    });
    view.contentDOM.dispatchEvent(mouseEvent);

    // Wait for async goto-definition chain: requestDefinition -> onNavigate -> bridge.openFile -> store.openFile
    await vi.waitFor(() => {
      expect(openFileSpy).toHaveBeenCalledWith('/project/src/mount.ri');
    });

    // After the bridge call resolves, the store should have switched to the target file
    await vi.waitFor(() => {
      expect(store.state.activeFile).toBe(file2.path);
    });
  });
});

describe('Editor cross-file goto-definition cleanup (B1)', () => {
  it('unmount during in-flight bridgeOpenFile does not dispatch to destroyed view', async () => {
    const store = setupStore([file1]);
    store.setActiveFile(file1.path);

    const crossFileLocation = {
      uri: 'file:///project/src/mount.ri',
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } },
    };

    mockInvoke.mockImplementation(async (_cmd: string, args: any) => {
      const method = (args as any)?.method as string;
      if (method === 'initialize') return JSON.stringify({ capabilities: {} });
      if (method === 'textDocument/definition') return JSON.stringify(crossFileLocation);
      return undefined as any;
    });

    // Return a deferred promise so we can unmount while it's in-flight
    const deferredOpen = deferred<FileData>();
    vi.spyOn(bridge, 'openFile').mockReturnValue(deferredOpen.promise);

    const { unmount } = render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    vi.spyOn(view, 'posAtCoords').mockReturnValue(5);
    const dispatchSpy = vi.spyOn(view, 'dispatch');

    // Trigger Ctrl+Click → goto-definition → bridgeOpenFile (now pending)
    view.contentDOM.dispatchEvent(
      new MouseEvent('mousedown', { ctrlKey: true, clientX: 100, clientY: 50, bubbles: true }),
    );

    // Wait for bridgeOpenFile to be called
    await vi.waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalled();
    });

    // Unmount component — sets destroyed=true, calls view.destroy()
    unmount();

    // Now resolve the in-flight bridgeOpenFile
    deferredOpen.resolve(file2);
    await vi.advanceTimersByTimeAsync(10);

    // view.dispatch should NOT have been called after unmount
    // (the destroyed guard prevents dispatch to the destroyed view)
    expect(dispatchSpy).not.toHaveBeenCalled();
  });
});

describe('Editor extensions', () => {
  it('renders line numbers gutter (.cm-lineNumbers)', () => {
    const store = setupStore();
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const lineNumbers = container.querySelector('.cm-lineNumbers');
    expect(lineNumbers).not.toBeNull();
  });

  it('closeBrackets extension is loaded (inputHandler registered)', () => {
    const store = setupStore([{ path: '/test.ri', content: '' }]);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // closeBrackets registers an EditorView.inputHandler facet entry.
    // In JSDOM, the native input pipeline (beforeinput) doesn't work,
    // so we verify the extension is loaded by checking the facet.
    const handlers = view.state.facet(EditorView.inputHandler);
    expect(handlers.length).toBeGreaterThan(0);
  });

  it('Ctrl+F opens search panel (.cm-search)', () => {
    const store = setupStore();
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Dispatch Ctrl+F to open search panel
    const event = new KeyboardEvent('keydown', {
      key: 'f',
      code: 'KeyF',
      ctrlKey: true,
      bubbles: true,
    });
    view.contentDOM.dispatchEvent(event);

    // CodeMirror search panel should now be rendered
    const searchPanel = container.querySelector('.cm-search');
    expect(searchPanel).not.toBeNull();
  });
});

describe('Editor cleanup race condition (RC-05)', () => {
  it('cleanup during in-flight file switch prevents phantom didOpen', async () => {
    const store = setupStore([file1, file2]);
    store.setActiveFile(file1.path);

    const lspCalls: string[] = [];
    const deferredClose = deferred<void>();
    let closeCaptured = false;

    mockInvoke.mockImplementation(async (_cmd: string, args: any) => {
      const method = (args as any)?.method as string;
      if (method === 'initialize') return JSON.stringify({ capabilities: {} });

      const params = JSON.parse((args as any)?.params ?? '{}');
      const uri = params?.textDocument?.uri ?? '';

      if (method?.startsWith('textDocument/did')) {
        lspCalls.push(`${method}|${uri}`);
      }

      // Return a controllable promise for the file-switch's didClose(bracket)
      // so we can interleave unmount while the chain is mid-flight
      if (
        method === 'textDocument/didClose' &&
        uri.includes('bracket') &&
        !closeCaptured
      ) {
        closeCaptured = true;
        return deferredClose.promise as any;
      }

      return undefined as any;
    });

    const { unmount } = render(() => <Editor store={store} />);

    // Wait for initial LSP setup (initialize → initialized → didOpen)
    await vi.waitFor(() => {
      expect(lspCalls.some((c) => c.includes('didOpen'))).toBe(true);
    });
    lspCalls.length = 0;

    // Trigger file switch file1 → file2
    store.setActiveFile(file2.path);

    // Wait for didClose(bracket) to be called — it's now pending via deferred
    await vi.waitFor(() => {
      expect(closeCaptured).toBe(true);
    });

    // Resolve didClose(bracket) and IMMEDIATELY unmount before the chain's
    // didOpen microtask gets a chance to run — this creates the race condition
    deferredClose.resolve();
    unmount();

    // Flush all microtasks to let pending promise chains settle
    for (let i = 0; i < 20; i++) await Promise.resolve();

    // With the fix (destroyed flag + chained cleanup):
    //   The chain's didOpen is SKIPPED (destroyed=true), then cleanup's
    //   chained didClose(mount) fires.
    //   Result: [didClose(bracket), didClose(mount)]
    //
    // BUG (no destroyed flag):
    //   Cleanup fires didClose(mount) directly, then the chain's didOpen(mount)
    //   fires afterward — leaving a phantom open document in the LSP server.
    //   Result: [didClose(bracket), didClose(mount), didOpen(mount)]

    // No phantom didOpen should be present
    const didOpenCalls = lspCalls.filter((c) => c.includes('didOpen'));
    expect(didOpenCalls).toHaveLength(0);

    // Last call should be didClose for the file active at cleanup time
    const lastCall = lspCalls[lspCalls.length - 1];
    expect(lastCall).toContain('textDocument/didClose');
    expect(lastCall).toContain('mount.ri');
  });

  it('cleanup fires didClose for current URI when no file switch is in flight', async () => {
    const store = setupStore([file1]);
    store.setActiveFile(file1.path);

    const lspCalls: string[] = [];
    mockInvoke.mockImplementation(async (_cmd: string, args: any) => {
      const method = (args as any)?.method as string;
      if (method === 'initialize') return JSON.stringify({ capabilities: {} });

      const params = JSON.parse((args as any)?.params ?? '{}');
      const uri = params?.textDocument?.uri ?? '';

      if (method?.startsWith('textDocument/did')) {
        lspCalls.push(`${method}|${uri}`);
      }

      return undefined as any;
    });

    const { unmount } = render(() => <Editor store={store} />);

    // Wait for initial LSP setup
    await vi.waitFor(() => {
      expect(lspCalls.some((c) => c.includes('didOpen'))).toBe(true);
    });
    lspCalls.length = 0;

    // Unmount with no file switch in progress
    unmount();

    // Flush microtasks (cleanup's chained didClose)
    for (let i = 0; i < 10; i++) await Promise.resolve();

    // Should have exactly one didClose for the active file
    const closeCalls = lspCalls.filter((c) => c.includes('didClose'));
    expect(closeCalls).toHaveLength(1);
    expect(closeCalls[0]).toContain('bracket.ri');
  });
});

describe('Editor open (Ctrl+O)', () => {
  it('Ctrl+O keydown dispatched on editor contentDOM calls props.onOpen', () => {
    const store = setupStore();
    const onOpen = vi.fn();
    render(() => <Editor store={store} onOpen={onOpen} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    const event = new KeyboardEvent('keydown', {
      key: 'o',
      code: 'KeyO',
      ctrlKey: true,
      bubbles: true,
    });
    view.contentDOM.dispatchEvent(event);

    expect(onOpen).toHaveBeenCalledTimes(1);
  });
});

describe('Editor Mod-s aborts when file is externally changed', () => {
  it('(a) Mod-s routes to onSaveConflict (NOT onError) when active file is externally changed', () => {
    const store = setupStore();
    const onError = vi.fn();
    const onSaveConflict = vi.fn();
    const saveSpy = vi.spyOn(bridge, 'saveFile').mockResolvedValue(undefined);
    store.markExternallyChanged(file1.path);
    render(() => <Editor store={store} onError={onError} onSaveConflict={onSaveConflict} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    const event = new KeyboardEvent('keydown', {
      key: 's',
      code: 'KeyS',
      ctrlKey: true,
      bubbles: true,
    });
    view.contentDOM.dispatchEvent(event);

    // saveFile must NOT be called
    expect(saveSpy).not.toHaveBeenCalled();
    // onSaveConflict must be called exactly once with the matching FileData
    expect(onSaveConflict).toHaveBeenCalledOnce();
    expect(onSaveConflict).toHaveBeenCalledWith(file1);
    // onError must NOT be called — the externally-changed branch no longer
    // delegates to onError; it delegates to onSaveConflict instead.
    expect(onError).not.toHaveBeenCalled();
  });

  it('(b) after clearExternallyChanged, Mod-s DOES call saveFile', async () => {
    const store = setupStore();
    const saveSpy = vi.spyOn(bridge, 'saveFile').mockResolvedValue(undefined);
    store.markExternallyChanged(file1.path);
    store.clearExternallyChanged(file1.path);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    const event = new KeyboardEvent('keydown', {
      key: 's',
      code: 'KeyS',
      ctrlKey: true,
      bubbles: true,
    });
    view.contentDOM.dispatchEvent(event);

    expect(saveSpy).toHaveBeenCalledWith(file1.path, file1.content);
  });

  it('(c) regression: for a non-externally-changed file, Mod-s still saves normally (onSaveConflict is NOT called)', async () => {
    const store = setupStore();
    store.markDirty(file1.path);
    const saveSpy = vi.spyOn(bridge, 'saveFile').mockResolvedValue(undefined);
    const onSaveConflict = vi.fn();
    render(() => <Editor store={store} onSaveConflict={onSaveConflict} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    const event = new KeyboardEvent('keydown', {
      key: 's',
      code: 'KeyS',
      ctrlKey: true,
      bubbles: true,
    });
    view.contentDOM.dispatchEvent(event);

    // saveFile IS called — happy path unchanged by adding onSaveConflict prop
    expect(saveSpy).toHaveBeenCalledWith(file1.path, file1.content);
    // onSaveConflict is NOT called for a clean save
    expect(onSaveConflict).not.toHaveBeenCalled();
  });
});

describe('Editor Mod-s when file is not in store', () => {
  let consoleErrorSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
  });

  afterEach(() => {
    consoleErrorSpy.mockRestore();
  });

  it('does NOT call saveFile, does NOT call onError, but DOES emit console.error when activeFile is not in openFiles', () => {
    const store = setupStore([file1]);
    // Set activeFile to a path that is NOT in openFiles
    store.setActiveFile('/project/src/missing.ri');
    const onError = vi.fn();
    const saveSpy = vi.spyOn(bridge, 'saveFile').mockResolvedValue(undefined);
    render(() => <Editor store={store} onError={onError} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    const event = new KeyboardEvent('keydown', {
      key: 's',
      code: 'KeyS',
      ctrlKey: true,
      bubbles: true,
    });
    view.contentDOM.dispatchEvent(event);

    // saveFile should NOT be called — the file isn't in the store
    expect(saveSpy).not.toHaveBeenCalled();
    // onError must NOT be called — not-found is a silent invariant breach, not user-actionable
    expect(onError).not.toHaveBeenCalled();
    // The diagnostic breadcrumb must still be emitted for debugging
    expect(consoleErrorSpy).toHaveBeenCalledWith(
      'Save aborted: file not in store',
      '/project/src/missing.ri',
    );
  });
});

describe('Editor theme integration', () => {
  it('mounts .cm-editor element successfully with reify theme', () => {
    const store = setupStore();
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    expect(container.querySelector('.cm-editor')).not.toBeNull();
  });

  it('Editor.tsx imports reifyEditorTheme and reifyHighlightStyle from editorTheme', async () => {
    // Verify the module source uses editorTheme imports, not defaultHighlightStyle
    const editorSrc = await import('../editor/Editor?raw');
    expect(editorSrc.default).toContain('editorTheme');
    expect(editorSrc.default).not.toContain('defaultHighlightStyle');
  });
});

describe('Editor view stays in sync with store content', () => {
  it('updateFileContent for active file propagates to CodeMirror view', async () => {
    const fileA: FileData = { path: '/a.ri', content: 'OLD' };
    const store = setupStore([fileA]);

    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Verify initial content is rendered correctly
    expect(view.state.doc.toString()).toBe('OLD');
    expect(container.querySelector('.cm-content')!.textContent).toContain('OLD');

    // External store update — simulates auto-reload or handleReload writing to the store
    store.updateFileContent('/a.ri', 'NEW');

    // The content-sync createEffect should fire and dispatch the new content into the view
    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe('NEW');
    });

    expect(container.querySelector('.cm-content')!.textContent).toContain('NEW');
  });

  it('user typing does NOT cause the sync effect to overwrite the typed content', () => {
    const fileA: FileData = { path: '/a.ri', content: 'INITIAL' };
    const store = setupStore([fileA]);
    vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);

    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // User inserts 'X' at position 0 — changes the view but NOT the store's file.content
    view.dispatch({ changes: { from: 0, insert: 'X' } });

    // Advance past the debounce — triggers updateSource (backend), not store updateFileContent
    vi.advanceTimersByTime(EDITOR_DEBOUNCE_MS);

    // The view's typed edit must survive — sync effect must NOT have overwritten it
    // (effect only re-runs when store's file.content changes, not when the view changes)
    expect(view.state.doc.toString()).toBe('XINITIAL');

    // Store content is unchanged — typing never calls updateFileContent
    const openFile = store.state.openFiles.find((f) => f.path === '/a.ri');
    expect(openFile!.content).toBe('INITIAL');
  });

  it('sync dispatch does NOT call markDirty or updateSource (non-user origin)', async () => {
    // Regression guard: the sync createEffect's dispatch must be annotated as
    // 'sync.external' so the updateListener bails before calling markDirty +
    // updateSource. Without the annotation the auto-reload echoes back to the
    // backend as a phantom user edit and immediately re-marks the file dirty.
    const fileA: FileData = { path: '/a.ri', content: 'OLD' };
    const store = setupStore([fileA]);

    const markDirtySpy = vi.spyOn(store, 'markDirty');
    const updateSourceSpy = vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);

    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Verify initial state — file is NOT dirty
    expect(store.state.dirtyFiles).not.toContain('/a.ri');

    // External store update — simulates auto-reload writing to the store
    store.updateFileContent('/a.ri', 'NEW');

    // Wait for the sync effect to dispatch the new content into the view
    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe('NEW');
    });

    // Advance past the debounce to catch any delayed updateSource calls
    vi.advanceTimersByTime(EDITOR_DEBOUNCE_MS + 100);

    // (a) dirtyFiles must NOT contain '/a.ri' — the sync dispatch must not mark the file dirty
    expect(store.state.dirtyFiles).not.toContain('/a.ri');

    // (b) markDirty was NOT called as a result of the sync transaction
    expect(markDirtySpy).not.toHaveBeenCalled();

    // (c) updateSource bridge call was NOT made — the auto-reload must not echo back to backend
    expect(updateSourceSpy).not.toHaveBeenCalled();
  });
});

describe('Editor sync dispatch excluded from undo history', () => {
  it('external auto-reload is excluded from CodeMirror undo history (Ctrl+Z cannot revive stale buffer)', async () => {
    // Regression guard: the sync dispatch must include Transaction.addToHistory.of(false)
    // so that undo after a silent auto-reload cannot revert the buffer to the
    // pre-reload (stale) content.
    //
    // Failure mode (without addToHistory.of(false)):
    //   1. user types 'X' → doc = 'XOLD'  (enters undo history)
    //   2. external reload → doc = 'RELOADED' (also enters undo history — BUG)
    //   3. undo → doc = 'XOLD'  ← user accidentally restores stale pre-reload state
    //
    // Expected (with addToHistory.of(false)):
    //   Steps 1-2 same, but reload does NOT enter history.
    //   3. undo → NOT 'XOLD' (the sync entry is skipped)
    const fileA: FileData = { path: '/a.ri', content: 'OLD' };
    const store = setupStore([fileA]);
    vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);

    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    expect(view.state.doc.toString()).toBe('OLD');

    // Step 1: user types 'X' — this IS added to undo history
    view.dispatch({ changes: { from: 0, insert: 'X' } });
    expect(view.state.doc.toString()).toBe('XOLD');

    // Step 2: external auto-reload sets content to 'RELOADED' — must NOT enter undo history
    store.updateFileContent('/a.ri', 'RELOADED');
    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe('RELOADED');
    });

    // Step 3: Undo via the imported `undo` command (same as what historyKeymap binds to Mod-z).
    // Without addToHistory.of(false), the sync transaction IS in the undo stack, so
    // undo would yield 'XOLD'. With it excluded, undo is at most a no-op or reverts 'X'.
    // The critical invariant: NEVER 'XOLD'.
    undo(view);

    // After undo, doc must not be the intermediate 'XOLD' state (which would
    // mean the sync transaction was undoable and the user just reverted the reload).
    expect(view.state.doc.toString()).not.toBe('XOLD');
  });
});

describe('Editor structural folding', () => {
  const foldFile: FileData = {
    path: '/fold.ri',
    content: 'structure S {\n  param a = 1\n  param b = 2\n}',
  };

  it('foldGutter is wired (.cm-foldGutter present in DOM)', () => {
    const store = setupStore([foldFile]);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const foldGutter = container.querySelector('.cm-foldGutter');
    expect(foldGutter).not.toBeNull();
  });

  it('foldAll produces non-empty foldedRanges; unfoldAll clears them', () => {
    const store = setupStore([foldFile]);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Initially no folded ranges
    let count = 0;
    foldedRanges(view.state).between(0, view.state.doc.length, () => { count++; });
    expect(count).toBe(0);

    // After foldAll, at least one range should be folded
    foldAll(view);
    count = 0;
    foldedRanges(view.state).between(0, view.state.doc.length, () => { count++; });
    expect(count).toBeGreaterThan(0);

    // After unfoldAll, no folded ranges remain
    unfoldAll(view);
    count = 0;
    foldedRanges(view.state).between(0, view.state.doc.length, () => { count++; });
    expect(count).toBe(0);
  });

  it('foldKeymap bindings Ctrl-Shift-[ and Ctrl-Alt-[ are registered in keymap facet', () => {
    const store = setupStore([foldFile]);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    const bindings = view.state.facet(keymap).flat();
    expect(bindings.some((b) => b.key === 'Ctrl-Shift-[')).toBe(true);
    expect(bindings.some((b) => b.key === 'Ctrl-Alt-[')).toBe(true);
  });
});

describe('Editor Mod-s exhaustiveness for SaveBlockedReason', () => {
  it('default arm logs to console.error (and does NOT fall through to saveFile) when canSave returns an unhandled reason', () => {
    const store = setupStore();
    vi.spyOn(store, 'canSave').mockReturnValue({ ok: false, reason: 'phantom-future-reason' } as any);
    const saveSpy = vi.spyOn(bridge, 'saveFile').mockResolvedValue(undefined);
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    const onError = vi.fn();
    try {
      render(() => <Editor store={store} onError={onError} />);
      const view = getEditorView(screen.getByTestId('editor-container'));
      view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 's', code: 'KeyS', ctrlKey: true, bubbles: true }));
      // Defense-in-depth: saveSpy is never called regardless (no `file` property on the mock result),
      // but it catches future regressions where the default arm falls through to saveFile.
      expect(saveSpy).not.toHaveBeenCalled();
      expect(consoleSpy).toHaveBeenCalledWith(
        expect.stringContaining('unhandled save-blocked reason'),
        'phantom-future-reason',
      );
      expect(onError).toHaveBeenCalledWith(expect.stringContaining('Save failed'));
    } finally {
      consoleSpy.mockRestore();
    }
  });
});

describe('Editor navigation history', () => {
  // file1: 'structure Bracket {\n  param width = 80mm\n}'
  // Line 2 (1-based) starts at offset 20; char 8 → offset 28.
  const ORIGIN_OFFSET = 5;
  const DEF_OFFSET = 28; // line2.from(20) + lspChar(8) = 28

  const sameFileDefForNav = {
    uri: 'file:///project/src/bracket.ri',
    range: { start: { line: 1, character: 8 }, end: { line: 1, character: 13 } },
  };

  function setupNavStore() {
    const store = setupStore([file1]);
    store.setActiveFile(file1.path);
    mockInvoke.mockImplementation(async (_cmd: string, args: any) => {
      const method = (args as any)?.method as string;
      if (method === 'initialize') return JSON.stringify({ capabilities: {} });
      if (method === 'textDocument/definition') return JSON.stringify(sameFileDefForNav);
      return undefined as any;
    });
    return store;
  }

  it('(a) Alt+ArrowLeft after F12 same-file jump returns cursor to pre-jump origin', async () => {
    const store = setupNavStore();
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Place cursor at origin
    view.dispatch({ selection: { anchor: ORIGIN_OFFSET } });
    expect(view.state.selection.main.head).toBe(ORIGIN_OFFSET);

    // F12: jump to definition (async)
    view.contentDOM.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F12', code: 'F12', bubbles: true }),
    );
    await vi.waitFor(() => expect(view.state.selection.main.head).toBe(DEF_OFFSET));

    // Alt+ArrowLeft: navigate back to origin
    view.contentDOM.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowLeft', altKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(view.state.selection.main.head).toBe(ORIGIN_OFFSET));
  });

  it('(b) Alt+ArrowRight after Alt+ArrowLeft re-advances cursor to definition offset', async () => {
    const store = setupNavStore();
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    view.dispatch({ selection: { anchor: ORIGIN_OFFSET } });

    // F12: jump to definition
    view.contentDOM.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F12', code: 'F12', bubbles: true }),
    );
    await vi.waitFor(() => expect(view.state.selection.main.head).toBe(DEF_OFFSET));

    // Alt+ArrowLeft: back to origin
    view.contentDOM.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowLeft', altKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(view.state.selection.main.head).toBe(ORIGIN_OFFSET));

    // Alt+ArrowRight: re-advance to definition
    view.contentDOM.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowRight', altKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(view.state.selection.main.head).toBe(DEF_OFFSET));
  });

  it('(c) scrollToLocation reveal pushes to nav history; Alt+ArrowLeft returns to origin', () => {
    const store = setupStore([file1]);
    store.setActiveFile(file1.path);
    const [scrollTo, setScrollTo] = createSignal<SourceLocation | null>(null);
    render(() => <Editor store={store} scrollToLocation={scrollTo} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Place cursor at origin
    view.dispatch({ selection: { anchor: ORIGIN_OFFSET } });
    expect(view.state.selection.main.head).toBe(ORIGIN_OFFSET);

    // Cross-pane reveal: line 2, column 9 (1-based) → anchor = 20 + 8 = 28
    setScrollTo({ file_path: file1.path, line: 2, column: 9, end_line: 2, end_column: 9 });
    expect(view.state.selection.main.head).toBe(DEF_OFFSET);

    // Alt+ArrowLeft: nav back to origin
    view.contentDOM.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowLeft', altKey: true, bubbles: true }),
    );
    expect(view.state.selection.main.head).toBe(ORIGIN_OFFSET);
  });
});

describe('Editor F12 go-to-definition', () => {
  it('F12 on cursor position dispatches cursor to same-file definition offset', async () => {
    const store = setupStore([file1]);
    store.setActiveFile(file1.path);

    // file1: 'structure Bracket {\n  param width = 80mm\n}'
    // Same-file def at LSP line 1 (0-based = CM line 2), char 8 → CM offset 28
    // Line 2 starts at offset 20 (line 1 = 19 chars + '\n'); 20 + 8 = 28
    const sameFileDef = {
      uri: 'file:///project/src/bracket.ri',
      range: { start: { line: 1, character: 8 }, end: { line: 1, character: 13 } },
    };
    mockInvoke.mockImplementation(async (_cmd: string, args: any) => {
      const method = (args as any)?.method as string;
      if (method === 'initialize') return JSON.stringify({ capabilities: {} });
      if (method === 'textDocument/definition') return JSON.stringify(sameFileDef);
      return undefined as any;
    });

    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Place cursor at origin offset 5 (within "structure")
    view.dispatch({ selection: { anchor: 5 } });
    expect(view.state.selection.main.head).toBe(5);

    // Dispatch F12 keydown on contentDOM
    view.contentDOM.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F12', code: 'F12', bubbles: true }),
    );

    // Wait for async goto-definition to resolve and cursor to land at target offset 28
    await vi.waitFor(() => {
      expect(view.state.selection.main.head).toBe(28);
    });
  });
});

describe('Editor compile diagnostics', () => {
  // A compile Error diagnostic whose file_path matches file1
  const errorDiagForActiveFile: DiagnosticInfo = {
    file_path: file1.path,
    line: 1,
    column: 11,
    end_line: 1,
    end_column: 20,
    severity: 'Error',
    message: 'unresolved name: rot_to_z',
    code: null,
  };

  it('(a) rendering with an active-file compile diagnostic shows count === 1 with error severity', () => {
    setupListenCapture();
    const store = setupStore([file1]);
    render(() => (
      <Editor store={store} compileDiagnostics={[errorDiagForActiveFile]} />
    ));
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    expect(diagnosticCount(view.state)).toBe(1);

    const severities: string[] = [];
    forEachDiagnostic(view.state, (d) => severities.push(d.severity));
    expect(severities).toContain('error');
  });

  it('(b) a compile diagnostic for a DIFFERENT file is NOT applied (count stays 0)', () => {
    setupListenCapture();
    const store = setupStore([file1]);
    render(() => (
      <Editor
        store={store}
        compileDiagnostics={[{ ...errorDiagForActiveFile, file_path: '/project/src/other.ri' }]}
      />
    ));
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    expect(diagnosticCount(view.state)).toBe(0);
  });

  it('(c) compile diagnostic + LSP diagnostic coexist (count === 2, neither clobbers the other)', () => {
    const getHandler = setupListenCapture();
    const store = setupStore([file1]);
    render(() => (
      <Editor store={store} compileDiagnostics={[errorDiagForActiveFile]} />
    ));
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Compile diagnostic should already be applied (1)
    expect(diagnosticCount(view.state)).toBe(1);

    // Fire an LSP diagnostics event for the active file
    const handler = getHandler();
    expect(handler).toBeDefined();
    handler!({
      payload: {
        uri: 'file:///project/src/bracket.ri',
        diagnostics: [
          {
            range: { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } },
            severity: 2,
            message: 'lsp warning',
          },
        ],
      },
    });

    // Both should coexist: count === 2
    expect(diagnosticCount(view.state)).toBe(2);
  });

  it('(d) clearing compileDiagnostics prop removes the compile squiggle', () => {
    setupListenCapture();
    const store = setupStore([file1]);
    const [diags, setDiags] = createSignal<DiagnosticInfo[]>([errorDiagForActiveFile]);
    render(() => <Editor store={store} compileDiagnostics={diags()} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    expect(diagnosticCount(view.state)).toBe(1);

    // Clear the compile diagnostics
    setDiags([]);

    expect(diagnosticCount(view.state)).toBe(0);
  });

  it('(e) CRASH (same-file) — stale LSP offset beyond a shrunk doc must not throw when the compile effect re-dispatches', () => {
    // Regression for esc-4252-74: applyMergedDiagnostics filtered only
    // compileCmDiagnostics against the live doc length and spread lspCmDiagnostics
    // unguarded. Same-file race: the LSP listener stores offsets against the long
    // doc → the user types/deletes so the view doc shrinks (typing mutates the view
    // directly; neither diagnostic effect re-runs on keystrokes) → a compileDiagnostics
    // update fires the compile effect → applyMergedDiagnostics re-dispatches the now
    // out-of-range lspCmDiagnostics into the shrunk doc → CodeMirror RangeSet build
    // throws. The fix filters the merged union, so neither channel can dispatch stale
    // ranges.
    const getHandler = setupListenCapture();
    const store = setupStore([file1]);
    const [diags, setDiags] = createSignal<DiagnosticInfo[]>([]);
    render(() => <Editor store={store} compileDiagnostics={diags()} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    const handler = getHandler();
    expect(handler).toBeDefined();

    // LSP diagnostic for file1 at LSP line 1 (0-based) chars 0-5 → CM offsets [20, 25],
    // valid in file1's 42-char doc.
    handler!({
      payload: {
        uri: 'file:///project/src/bracket.ri',
        diagnostics: [{
          range: { start: { line: 1, character: 0 }, end: { line: 1, character: 5 } },
          severity: 1,
          message: 'file1 deep error',
        }],
      },
    });
    expect(diagnosticCount(view.state)).toBe(1);

    // Shrink the doc directly (simulates the user deleting text). The diagnostic
    // effects do NOT re-run, so lspCmDiagnostics still holds the stale [20, 25].
    view.dispatch({ changes: { from: 5, to: view.state.doc.length } });
    expect(view.state.doc.length).toBe(5);

    // A compileDiagnostics update (fresh empty array → new reference) re-runs the
    // compile effect → applyMergedDiagnostics. Under the bug the stale LSP [20, 25] is
    // dispatched into the 5-char doc and throws. After the fix the merged union is
    // filtered, so this is a no-op (no crash) and the stale LSP squiggle is dropped.
    expect(() => setDiags([])).not.toThrow();
    expect(diagnosticCount(getEditorView(container).state)).toBe(0);
  });
});

describe('Editor compile diagnostics: stale LSP cleared on file switch', () => {
  it('(a) FLASH — file2 LSP squiggle does NOT leak into file1 after switch', () => {
    const getHandler = setupListenCapture();
    const store = setupStore([file2, file1]);
    store.setActiveFile(file2.path);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    const handler = getHandler();
    expect(handler).toBeDefined();

    // Fire LSP diagnostics for file2 URI at range [0, 5]
    handler!({
      payload: {
        uri: 'file:///project/src/mount.ri',
        diagnostics: [{
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } },
          severity: 1,
          message: 'stale diagnostic from file2',
        }],
      },
    });
    expect(diagnosticCount(view.state)).toBe(1);

    // Switch to file1 — stale lspCmDiagnostics [0,5] must NOT be re-dispatched.
    // Under the bug the compile effect calls applyMergedDiagnostics() with the un-cleared
    // lspCmDiagnostics, so count stays 1 instead of going to 0.
    store.setActiveFile(file1.path);

    expect(diagnosticCount(getEditorView(container).state)).toBe(0);
  });

  it('(b) CRASH — stale LSP offset beyond new doc length must not throw or leave squiggle', () => {
    const getHandler = setupListenCapture();
    const store = setupStore([file1, file2]);
    store.setActiveFile(file1.path);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    const handler = getHandler();
    expect(handler).toBeDefined();

    // LSP diagnostic for file1 at LSP line 1 (0-based), chars 0-5.
    // lspDiagnosticToCodeMirror maps this to CM offsets from=20, to=25
    // (file1 line-2 starts at offset 20 in the 42-char doc).
    // [20, 25] is valid in file1 but exceeds file2's 18-char doc.
    handler!({
      payload: {
        uri: 'file:///project/src/bracket.ri',
        diagnostics: [{
          range: { start: { line: 1, character: 0 }, end: { line: 1, character: 5 } },
          severity: 1,
          message: 'file1 deep error',
        }],
      },
    });
    expect(diagnosticCount(view.state)).toBe(1);

    // Under the bug: the compile effect re-dispatches the stale [20, 25] into file2's
    // 18-char doc → CodeMirror RangeSet build throws a RangeError.
    // After the fix: lspCmDiagnostics is cleared in the file-switch effect before the
    // compile effect runs, so applyMergedDiagnostics dispatches [] — no crash.
    expect(() => store.setActiveFile(file2.path)).not.toThrow();

    // Stale squiggle must be absent in file2 view
    expect(diagnosticCount(getEditorView(container).state)).toBe(0);
  });

  it('(c) ROBUSTNESS — after file switch the LSP channel still applies new-file diagnostics', () => {
    const getHandler = setupListenCapture();
    const store = setupStore([file1, file2]);
    store.setActiveFile(file1.path);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');

    // Switch to file2 (lspCmDiagnostics must be cleared, not permanently disabled)
    store.setActiveFile(file2.path);

    const handler = getHandler();
    expect(handler).toBeDefined();

    // Fire LSP event for file2 at a valid range [0, 5] (file2 is 18 chars)
    handler!({
      payload: {
        uri: 'file:///project/src/mount.ri',
        diagnostics: [{
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } },
          severity: 2,
          message: 'file2 warning',
        }],
      },
    });

    // The LSP slot was cleared (not disabled) so the new-file event produces exactly 1 squiggle.
    expect(diagnosticCount(getEditorView(container).state)).toBe(1);
  });
});

describe('Editor F2 inline rename', () => {
  /** Flush the async prepareRename → UI chain (microtasks only; no fake-timer advance). */
  async function flushRenameChain() {
    for (let i = 0; i < 10; i++) await Promise.resolve();
  }

  it('renameable position: F2 opens the inline rename field pre-filled with the placeholder', async () => {
    const store = setupStore([file1]);
    store.setActiveFile(file1.path);

    mockInvoke.mockImplementation(async (_cmd: string, args: any) => {
      const method = (args as any)?.method as string;
      if (method === 'initialize') return JSON.stringify({ capabilities: {} });
      if (method === 'textDocument/prepareRename') {
        // file1 line 2 `  param width = 80mm` — `width` spans 0-based chars 8..13.
        return JSON.stringify({
          range: { start: { line: 1, character: 8 }, end: { line: 1, character: 13 } },
          placeholder: 'width',
        });
      }
      return undefined as any;
    });

    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Place the cursor on the `width` token before pressing F2.
    const line2 = view.state.doc.line(2);
    view.dispatch({ selection: { anchor: line2.from + 8 } });

    view.contentDOM.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F2', code: 'F2', bubbles: true }),
    );
    await flushRenameChain();

    // The inline field appears, pre-filled with the current name.
    const field = container.querySelector('[data-testid="rename-field"]') as HTMLInputElement | null;
    expect(field).not.toBeNull();
    expect(field!.value).toBe('width');
  });

  it('non-renameable position: F2 shows the refusal message and makes NO document change', async () => {
    const store = setupStore([file1]);
    store.setActiveFile(file1.path);
    const updateSpy = vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);

    mockInvoke.mockImplementation(async (_cmd: string, args: any) => {
      const method = (args as any)?.method as string;
      if (method === 'initialize') return JSON.stringify({ capabilities: {} });
      // Invariant-4 refusal: prepareRename returns null for a non-renameable position.
      if (method === 'textDocument/prepareRename') return JSON.stringify(null);
      return undefined as any;
    });

    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);
    const docBefore = view.state.doc.toString();

    // Cursor at offset 0 (on the `structure` keyword) — refused.
    view.contentDOM.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F2', code: 'F2', bubbles: true }),
    );
    await flushRenameChain();

    // The transient "can't rename here" message appears…
    expect(container.querySelector('[data-testid="rename-message"]')).not.toBeNull();
    // …no inline field opened, and the document is byte-for-byte unchanged.
    expect(container.querySelector('[data-testid="rename-field"]')).toBeNull();
    expect(view.state.doc.toString()).toBe(docBefore);

    // The refusal path performs zero edits → no debounced backend source update.
    vi.advanceTimersByTime(EDITOR_DEBOUNCE_MS + 100);
    expect(updateSpy).not.toHaveBeenCalled();
  });
});
