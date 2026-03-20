import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
import { EditorView } from '@codemirror/view';
import { createEditorStore } from '../stores/editorStore';
import * as bridge from '../bridge';
import type { FileData } from '../types';

// Mock Tauri API modules before importing Editor
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { Editor } from '../editor/Editor';

const file1: FileData = { path: '/project/src/bracket.ri', content: 'structure Bracket {\n  param width = 80mm\n}' };
const file2: FileData = { path: '/project/src/mount.ri', content: 'structure Mount {}' };

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
