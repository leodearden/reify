import { createStore } from 'solid-js/store';
import type { FileData } from '../types';

export interface EditorState {
  openFiles: FileData[];
  activeFile: string | null;
  dirtyFiles: string[];
  externallyChanged: string[];
  cursorPosition: { line: number; column: number } | null;
}

export function createEditorStore() {
  const [state, setState] = createStore<EditorState>({
    openFiles: [],
    activeFile: null,
    dirtyFiles: [],
    externallyChanged: [],
    cursorPosition: null,
  });

  function openFile(file: FileData) {
    const exists = state.openFiles.some((f) => f.path === file.path);
    if (!exists) {
      setState('openFiles', (files) => [...files, file]);
    } else {
      updateFileContent(file.path, file.content);
    }
    setState('activeFile', file.path);
  }

  function updateFileContent(path: string, content: string) {
    setState(
      'openFiles',
      (f) => f.path === path,
      'content',
      content,
    );
  }

  function closeFile(path: string) {
    const closedIndex = state.openFiles.findIndex((f) => f.path === path);
    const remaining = state.openFiles.filter((f) => f.path !== path);
    setState('openFiles', remaining);
    setState('dirtyFiles', (dirty) => dirty.filter((p) => p !== path));
    setState('externallyChanged', (ec) => ec.filter((p) => p !== path));
    if (state.activeFile === path) {
      const next = remaining[closedIndex] ?? remaining[closedIndex - 1] ?? null;
      setState('activeFile', next ? next.path : null);
    }
  }

  function setActiveFile(path: string) {
    setState('activeFile', path);
  }

  function markDirty(path: string) {
    if (!state.dirtyFiles.includes(path)) {
      setState('dirtyFiles', (dirty) => [...dirty, path]);
    }
  }

  function markClean(path: string) {
    // Called after a successful save or reload: the buffer now matches disk,
    // so both "user-typed-since-save" (dirtyFiles) and
    // "disk-diverged-since-load" (externallyChanged) flags are cleared.
    // This coupling is intentional — a save/reload always resolves both
    // conditions simultaneously.  Do NOT call markClean in a context where
    // only one flag should change; use clearExternallyChanged or markDirty
    // individually for narrower state transitions.
    setState('dirtyFiles', (dirty) => dirty.filter((p) => p !== path));
    setState('externallyChanged', (ec) => ec.filter((p) => p !== path));
  }

  function markExternallyChanged(path: string) {
    if (!state.externallyChanged.includes(path)) {
      setState('externallyChanged', (ec) => [...ec, path]);
    }
  }

  function clearExternallyChanged(path: string) {
    setState('externallyChanged', (ec) => ec.filter((p) => p !== path));
  }

  function clearAllExternallyChanged() {
    if (state.externallyChanged.length === 0) return;
    setState('externallyChanged', []);
  }

  function setCursorPosition(lineOrNull: number | null, column?: number) {
    if (lineOrNull === null) {
      setState('cursorPosition', null);
    } else {
      setState('cursorPosition', { line: lineOrNull, column: column ?? 0 });
    }
  }

  return { state, openFile, updateFileContent, closeFile, setActiveFile, markDirty, markClean, markExternallyChanged, clearExternallyChanged, clearAllExternallyChanged, setCursorPosition };
}
