import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
import { createEditorStore } from '../stores/editorStore';
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
