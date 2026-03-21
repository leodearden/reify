import { describe, it, expect } from 'vitest';
import { createRoot } from 'solid-js';
import { createEditorStore } from '../stores/editorStore';
import type { FileData } from '../types';

const file1: FileData = { path: 'bracket.ri', content: 'structure Bracket {}' };
const file2: FileData = { path: 'mount.ri', content: 'structure Mount {}' };

describe('editorStore', () => {
  it('has empty initial state', () => {
    createRoot((dispose) => {
      const { state } = createEditorStore();
      expect(state.openFiles).toEqual([]);
      expect(state.activeFile).toBeNull();
      expect(state.dirtyFiles).toEqual([]);
      expect(state.cursorPosition).toBeNull();
      dispose();
    });
  });

  it('openFile adds FileData and sets activeFile', () => {
    createRoot((dispose) => {
      const { state, openFile } = createEditorStore();
      openFile(file1);
      expect(state.openFiles).toHaveLength(1);
      expect(state.openFiles[0].path).toBe('bracket.ri');
      expect(state.activeFile).toBe('bracket.ri');
      dispose();
    });
  });

  it('opening duplicate file does not add twice', () => {
    createRoot((dispose) => {
      const { state, openFile } = createEditorStore();
      openFile(file1);
      openFile(file1);
      expect(state.openFiles).toHaveLength(1);
      dispose();
    });
  });

  it('closeFile removes and switches activeFile', () => {
    createRoot((dispose) => {
      const { state, openFile, closeFile } = createEditorStore();
      openFile(file1);
      openFile(file2);
      expect(state.activeFile).toBe('mount.ri');

      closeFile('mount.ri');
      expect(state.openFiles).toHaveLength(1);
      expect(state.activeFile).toBe('bracket.ri');

      closeFile('bracket.ri');
      expect(state.openFiles).toHaveLength(0);
      expect(state.activeFile).toBeNull();
      dispose();
    });
  });

  it('closeFile removes from dirtyFiles', () => {
    createRoot((dispose) => {
      const { state, openFile, closeFile, markDirty } = createEditorStore();
      openFile(file1);
      markDirty('bracket.ri');
      expect(state.dirtyFiles).toContain('bracket.ri');

      closeFile('bracket.ri');
      expect(state.dirtyFiles).not.toContain('bracket.ri');
      dispose();
    });
  });

  it('markDirty and markClean track dirty state', () => {
    createRoot((dispose) => {
      const { state, openFile, markDirty, markClean } = createEditorStore();
      openFile(file1);

      markDirty('bracket.ri');
      expect(state.dirtyFiles).toContain('bracket.ri');

      // Double-marking doesn't duplicate
      markDirty('bracket.ri');
      expect(state.dirtyFiles.filter((p) => p === 'bracket.ri')).toHaveLength(1);

      markClean('bracket.ri');
      expect(state.dirtyFiles).not.toContain('bracket.ri');
      dispose();
    });
  });

  it('setActiveFile switches active file', () => {
    createRoot((dispose) => {
      const { state, openFile, setActiveFile } = createEditorStore();
      openFile(file1);
      openFile(file2);
      expect(state.activeFile).toBe('mount.ri');

      setActiveFile('bracket.ri');
      expect(state.activeFile).toBe('bracket.ri');
      dispose();
    });
  });

  it('setCursorPosition updates cursor', () => {
    createRoot((dispose) => {
      const { state, setCursorPosition } = createEditorStore();
      setCursorPosition(10, 5);
      expect(state.cursorPosition).toEqual({ line: 10, column: 5 });

      setCursorPosition(null);
      expect(state.cursorPosition).toBeNull();
      dispose();
    });
  });

  it('setCursorPosition defaults column to 0 when not provided', () => {
    createRoot((dispose) => {
      const { state, setCursorPosition } = createEditorStore();
      setCursorPosition(10);
      expect(state.cursorPosition).toEqual({ line: 10, column: 0 });
      dispose();
    });
  });

  it('updateFileContent updates content of an already-open file', () => {
    createRoot((dispose) => {
      const { state, openFile, updateFileContent } = createEditorStore();
      openFile(file1);
      expect(state.openFiles[0].content).toBe('structure Bracket {}');

      updateFileContent('bracket.ri', 'structure Bracket { updated: true }');
      expect(state.openFiles[0].content).toBe('structure Bracket { updated: true }');
      dispose();
    });
  });
});
