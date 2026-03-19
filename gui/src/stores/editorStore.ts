import { createStore } from 'solid-js/store';
import type { FileData } from '../types';

export interface EditorState {
  openFiles: FileData[];
  activeFile: string | null;
  dirtyFiles: string[];
  cursorPosition: { line: number; column: number } | null;
}

export function createEditorStore() {
  const [state, setState] = createStore<EditorState>({
    openFiles: [],
    activeFile: null,
    dirtyFiles: [],
    cursorPosition: null,
  });

  function openFile(file: FileData) {
    const exists = state.openFiles.some((f) => f.path === file.path);
    if (!exists) {
      setState('openFiles', (files) => [...files, file]);
    }
    setState('activeFile', file.path);
  }

  function closeFile(path: string) {
    setState('openFiles', (files) => files.filter((f) => f.path !== path));
    setState('dirtyFiles', (dirty) => dirty.filter((p) => p !== path));
    if (state.activeFile === path) {
      const remaining = state.openFiles;
      setState('activeFile', remaining.length > 0 ? remaining[0].path : null);
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
    setState('dirtyFiles', (dirty) => dirty.filter((p) => p !== path));
  }

  function setCursorPosition(lineOrNull: number | null, column?: number) {
    if (lineOrNull === null) {
      setState('cursorPosition', null);
    } else {
      setState('cursorPosition', { line: lineOrNull, column: column! });
    }
  }

  return { state, openFile, closeFile, setActiveFile, markDirty, markClean, setCursorPosition };
}
