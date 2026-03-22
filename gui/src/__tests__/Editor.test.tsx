import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { EditorView } from '@codemirror/view';
import { undo } from '@codemirror/commands';
import { diagnosticCount } from '@codemirror/lint';
import { createEditorStore } from '../stores/editorStore';
import * as bridge from '../bridge';
import type { FileData, SourceLocation } from '../types';

// Mock Tauri API modules before importing Editor
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Editor } from '../editor/Editor';

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

  it('calls bridge.updateSource after 300ms debounce', () => {
    const store = setupStore();
    const updateSpy = vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    view.dispatch({ changes: { from: 0, insert: '// comment\n' } });

    // Not called immediately
    expect(updateSpy).not.toHaveBeenCalled();

    // After 300ms debounce
    vi.advanceTimersByTime(300);
    expect(updateSpy).toHaveBeenCalledWith(file1.path, expect.stringContaining('// comment'));
  });

  it('rapid edits collapse into a single updateSource call', () => {
    const store = setupStore();
    const updateSpy = vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined);
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
    vi.advanceTimersByTime(300);
    expect(updateSpy).toHaveBeenCalledTimes(1);
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
});

describe('Editor cursor tracking', () => {
  it('dispatching selection update sets cursor position in store', () => {
    const store = setupStore();
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Move cursor to line 2, column 5 (offset: line1 length + newline + 5)
    // file1 content: 'structure Bracket {\n  param width = 80mm\n}'
    //  line 1: 'structure Bracket {' (19 chars) + '\n' = offset 20
    //  line 2, col 5 => offset 25
    const offset = 25;
    view.dispatch({ selection: { anchor: offset } });

    expect(store.state.cursorPosition).not.toBeNull();
    expect(store.state.cursorPosition!.line).toBe(2);
    expect(store.state.cursorPosition!.column).toBe(5);
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
      file: file1.path,
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
  /** Capture the Tauri diagnostics event handler so we can fire events manually. */
  function setupListenCapture() {
    let diagnosticsHandler: ((event: { payload: any }) => void) | undefined;
    mockListen.mockImplementation(async (_event: any, handler: any) => {
      diagnosticsHandler = handler;
      return vi.fn(); // unlisten
    });
    return () => diagnosticsHandler;
  }

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
    const updateSpy = vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined);
    render(() => <Editor store={store} />);
    const container = screen.getByTestId('editor-container');
    const view = getEditorView(container);

    // Edit file1 (triggers debounce timer)
    view.dispatch({ changes: { from: 0, insert: '// edit\n' } });

    // Immediately switch to file2 (before 300ms elapses)
    store.setActiveFile(file2.path);

    // Advance timers past the debounce period
    vi.advanceTimersByTime(300);

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
