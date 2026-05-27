import { describe, it, expect } from 'vitest';
import { createRoot, createEffect, batch } from 'solid-js';
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

  // S3: stale closure — closeFile should compute fallback from local filtered list
  // Adjacent tab selection: closing last tab selects previous (B)
  it('closeFile selects adjacent tab when active file is closed (3 files, last closed)', () => {
    createRoot((dispose) => {
      const { state, openFile, closeFile } = createEditorStore();
      const fileA: FileData = { path: 'a.ri', content: 'a' };
      const fileB: FileData = { path: 'b.ri', content: 'b' };
      const fileC: FileData = { path: 'c.ri', content: 'c' };
      openFile(fileA);
      openFile(fileB);
      openFile(fileC);
      expect(state.activeFile).toBe('c.ri');

      closeFile('c.ri');
      // After closing C (last tab), remaining = [A, B] — select B (previous, at closedIndex-1)
      expect(state.openFiles).toHaveLength(2);
      expect(state.activeFile).toBe('b.ri');
      dispose();
    });
  });

  it('closeFile computes correct adjacent fallback inside batch()', () => {
    createRoot((dispose) => {
      const { state, openFile, closeFile } = createEditorStore();
      const fileA: FileData = { path: 'a.ri', content: 'a' };
      const fileB: FileData = { path: 'b.ri', content: 'b' };
      const fileC: FileData = { path: 'c.ri', content: 'c' };
      openFile(fileA);
      openFile(fileB);
      openFile(fileC);
      expect(state.activeFile).toBe('c.ri');

      // In a batched context, state reads after setState may return stale data
      batch(() => {
        closeFile('c.ri');
      });

      expect(state.openFiles).toHaveLength(2);
      expect(state.activeFile).toBe('b.ri');
      dispose();
    });
  });

  it('closeFile selects next tab when middle tab is closed', () => {
    createRoot((dispose) => {
      const { state, openFile, setActiveFile, closeFile } = createEditorStore();
      const fileA: FileData = { path: 'a.ri', content: 'a' };
      const fileB: FileData = { path: 'b.ri', content: 'b' };
      const fileC: FileData = { path: 'c.ri', content: 'c' };
      openFile(fileA);
      openFile(fileB);
      openFile(fileC);
      setActiveFile('b.ri');
      expect(state.activeFile).toBe('b.ri');

      closeFile('b.ri');
      // After closing B (middle), remaining = [A, C] — select C (next at same index)
      expect(state.openFiles).toHaveLength(2);
      expect(state.activeFile).toBe('c.ri');
      dispose();
    });
  });

  it('closeFile selects next tab when first tab is closed', () => {
    createRoot((dispose) => {
      const { state, openFile, setActiveFile, closeFile } = createEditorStore();
      const fileA: FileData = { path: 'a.ri', content: 'a' };
      const fileB: FileData = { path: 'b.ri', content: 'b' };
      openFile(fileA);
      openFile(fileB);
      setActiveFile('a.ri');
      expect(state.activeFile).toBe('a.ri');

      closeFile('a.ri');
      // After closing A (first tab), remaining = [B] — select B (next at index 0)
      expect(state.openFiles).toHaveLength(1);
      expect(state.activeFile).toBe('b.ri');
      dispose();
    });
  });

  // Step 7: markDirty idempotency at the reactive level
  // Proves that the includes-guard in markDirty elides setState when the path is
  // already in dirtyFiles, so the SolidJS reactive graph does NOT re-emit for
  // 1000 no-op markDirty calls.
  it('markDirty is idempotent on already-dirty path (no reactive re-emission)', async () => {
    let counter = 0;
    let storeRef!: ReturnType<typeof createEditorStore>;
    let disposeRef!: () => void;

    createRoot((dispose) => {
      disposeRef = dispose;
      storeRef = createEditorStore();
      storeRef.openFile(file1);

      // Track reactive emissions of dirtyFiles.length.
      // createEffect defers its first run to the next microtask, so counter
      // remains 0 until we await Promise.resolve() below.
      createEffect(() => {
        void storeRef.state.dirtyFiles.length; // establish reactive dependency
        counter++;
      });
    });

    // Flush the initial effect subscription (microtask scheduled by createEffect)
    await Promise.resolve();
    expect(counter).toBe(1); // effect ran once: initial subscribe (sees length=0)

    // First markDirty: clean → dirty — causes a real setState, re-queues the effect
    storeRef.markDirty(file1.path);
    await Promise.resolve();
    expect(counter).toBe(2); // effect fired once more (sees length=1)

    // 1000 more markDirty calls on the same already-dirty path.
    // The includes-guard in markDirty prevents setState, so the reactive graph
    // does NOT emit and the effect does NOT re-run.
    const dirtyFilesRefBeforeNoOps = storeRef.state.dirtyFiles;
    for (let i = 0; i < 1000; i++) {
      storeRef.markDirty(file1.path);
    }

    // Flush any pending effects (there should be none from the 1000 no-op calls)
    await Promise.resolve();

    // Counter must still be 2 — exactly one reactive emission per state transition,
    // zero emissions for idempotent markDirty calls
    expect(counter).toBe(2);

    // Structural assertion: the array still contains exactly one entry (no duplicates
    // were pushed). This directly proves the includes-guard works without relying on
    // effect scheduler semantics.
    expect(storeRef.state.dirtyFiles).toEqual([file1.path]);

    // Reference-identity assertion: the SolidJS store wraps arrays in a WeakMap-cached
    // Proxy. If setState were called (creating a new underlying array), the proxy
    // reference would change. Proving same reference confirms zero setState calls
    // during the 1000 no-op loop.
    expect(storeRef.state.dirtyFiles).toBe(dirtyFilesRefBeforeNoOps);

    disposeRef();
  });
});

// ─── externallyChanged tracking ─────────────────────────────────────────────

describe('editorStore externallyChanged', () => {
  it('(a) initial state.externallyChanged is []', () => {
    createRoot((dispose) => {
      const { state } = createEditorStore();
      expect(state.externallyChanged).toEqual([]);
      dispose();
    });
  });

  it('(b) markExternallyChanged(path) adds the path', () => {
    createRoot((dispose) => {
      const { state, openFile, markExternallyChanged } = createEditorStore();
      openFile(file1);
      markExternallyChanged('bracket.ri');
      expect(state.externallyChanged).toContain('bracket.ri');
      dispose();
    });
  });

  it('(c) markExternallyChanged is idempotent — duplicate call does not add a second entry', () => {
    createRoot((dispose) => {
      const { state, openFile, markExternallyChanged } = createEditorStore();
      openFile(file1);
      markExternallyChanged('bracket.ri');
      markExternallyChanged('bracket.ri');
      expect(state.externallyChanged.filter((p) => p === 'bracket.ri')).toHaveLength(1);
      dispose();
    });
  });

  it('(c) markExternallyChanged idempotency — 1000 calls cause no reactive re-emission', async () => {
    let counter = 0;
    let storeRef!: ReturnType<typeof createEditorStore>;
    let disposeRef!: () => void;

    createRoot((dispose) => {
      disposeRef = dispose;
      storeRef = createEditorStore();
      storeRef.openFile(file1);

      createEffect(() => {
        void storeRef.state.externallyChanged.length;
        counter++;
      });
    });

    // Flush the initial effect subscription
    await Promise.resolve();
    expect(counter).toBe(1);

    // First markExternallyChanged: new path → real setState → effect fires
    storeRef.markExternallyChanged(file1.path);
    await Promise.resolve();
    expect(counter).toBe(2);

    // 1000 more calls on the same already-external path — no setState
    const refBefore = storeRef.state.externallyChanged;
    for (let i = 0; i < 1000; i++) {
      storeRef.markExternallyChanged(file1.path);
    }
    await Promise.resolve();
    expect(counter).toBe(2); // no additional emissions
    expect(storeRef.state.externallyChanged).toEqual([file1.path]);
    expect(storeRef.state.externallyChanged).toBe(refBefore); // same proxy reference

    disposeRef();
  });

  it('(d) clearExternallyChanged(path) removes only that path', () => {
    createRoot((dispose) => {
      const { state, openFile, markExternallyChanged, clearExternallyChanged } = createEditorStore();
      openFile(file1);
      openFile(file2);
      markExternallyChanged('bracket.ri');
      markExternallyChanged('mount.ri');
      expect(state.externallyChanged).toContain('bracket.ri');
      expect(state.externallyChanged).toContain('mount.ri');

      clearExternallyChanged('bracket.ri');
      expect(state.externallyChanged).not.toContain('bracket.ri');
      expect(state.externallyChanged).toContain('mount.ri');
      dispose();
    });
  });

  it('(e) markClean(path) also clears externallyChanged for that path', () => {
    createRoot((dispose) => {
      const { state, openFile, markExternallyChanged, markClean } = createEditorStore();
      openFile(file1);
      markExternallyChanged('bracket.ri');
      expect(state.externallyChanged).toContain('bracket.ri');

      markClean('bracket.ri');
      expect(state.externallyChanged).not.toContain('bracket.ri');
      dispose();
    });
  });

  it('(f) closeFile(path) also clears externallyChanged for that path', () => {
    createRoot((dispose) => {
      const { state, openFile, closeFile, markExternallyChanged } = createEditorStore();
      openFile(file1);
      markExternallyChanged('bracket.ri');
      expect(state.externallyChanged).toContain('bracket.ri');

      closeFile('bracket.ri');
      expect(state.externallyChanged).not.toContain('bracket.ri');
      dispose();
    });
  });

  it('(g) markDirty and markExternallyChanged are independent — both can be true simultaneously', () => {
    createRoot((dispose) => {
      const { state, openFile, markDirty, markExternallyChanged } = createEditorStore();
      openFile(file1);

      markDirty('bracket.ri');
      markExternallyChanged('bracket.ri');
      expect(state.dirtyFiles).toContain('bracket.ri');
      expect(state.externallyChanged).toContain('bracket.ri');
      dispose();
    });
  });

  it('(g) markDirty does not imply markExternallyChanged', () => {
    createRoot((dispose) => {
      const { state, openFile, markDirty } = createEditorStore();
      openFile(file1);
      markDirty('bracket.ri');
      expect(state.dirtyFiles).toContain('bracket.ri');
      expect(state.externallyChanged).not.toContain('bracket.ri');
      dispose();
    });
  });

  it('(g) markExternallyChanged does not imply markDirty', () => {
    createRoot((dispose) => {
      const { state, openFile, markExternallyChanged } = createEditorStore();
      openFile(file1);
      markExternallyChanged('bracket.ri');
      expect(state.externallyChanged).toContain('bracket.ri');
      expect(state.dirtyFiles).not.toContain('bracket.ri');
      dispose();
    });
  });

  it('(h) clearAllExternallyChanged() resets externallyChanged to []', () => {
    createRoot((dispose) => {
      const { state, openFile, markExternallyChanged, clearAllExternallyChanged } = createEditorStore();
      openFile(file1);
      openFile(file2);
      markExternallyChanged('bracket.ri');
      markExternallyChanged('mount.ri');
      expect(state.externallyChanged).toContain('bracket.ri');
      expect(state.externallyChanged).toContain('mount.ri');

      clearAllExternallyChanged();
      expect(state.externallyChanged).toEqual([]);
      dispose();
    });
  });
});

// ─── editorStore canonical-key dedup (step-17) ───────────────────────────────

describe('editorStore openFile canonical-key dedup', () => {
  it('(a) file:///a/foo.ri then /a/foo.ri yields one tab; second call updates content', () => {
    createRoot((dispose) => {
      const { state, openFile } = createEditorStore();
      openFile({ path: 'file:///a/foo.ri', content: 'v1' });
      openFile({ path: '/a/foo.ri', content: 'v2' });
      expect(state.openFiles).toHaveLength(1);
      expect(state.openFiles[0].content).toBe('v2');
      dispose();
    });
  });

  it('(b) /a/./b/foo.ri then /a/b/foo.ri yields one tab', () => {
    createRoot((dispose) => {
      const { state, openFile } = createEditorStore();
      openFile({ path: '/a/./b/foo.ri', content: 'v1' });
      openFile({ path: '/a/b/foo.ri', content: 'v2' });
      expect(state.openFiles).toHaveLength(1);
      dispose();
    });
  });

  it('(c) stored path is the canonical form', () => {
    createRoot((dispose) => {
      const { state, openFile } = createEditorStore();
      openFile({ path: '/a/./b/foo.ri', content: 'x' });
      expect(state.openFiles[0].path).toBe('/a/b/foo.ri');
      dispose();
    });
  });

  it('(d) closeFile with file:// form closes tab opened with bare path', () => {
    createRoot((dispose) => {
      const { state, openFile, closeFile } = createEditorStore();
      openFile({ path: '/a/foo.ri', content: 'x' });
      closeFile('file:///a/foo.ri');
      expect(state.openFiles).toHaveLength(0);
      dispose();
    });
  });

  it('(d) markDirty with non-canonical path marks the canonicalized tab', () => {
    createRoot((dispose) => {
      const { state, openFile, markDirty } = createEditorStore();
      openFile({ path: '/a/b/foo.ri', content: 'x' });
      markDirty('file:///a/b/foo.ri');
      expect(state.dirtyFiles).toContain('/a/b/foo.ri');
      dispose();
    });
  });

  it('(d) setActiveFile with non-canonical path activates the canonical tab', () => {
    createRoot((dispose) => {
      const { state, openFile, setActiveFile } = createEditorStore();
      const f1: FileData = { path: '/a/b/foo.ri', content: 'x' };
      const f2: FileData = { path: '/a/b/bar.ri', content: 'y' };
      openFile(f1);
      openFile(f2);
      setActiveFile('file:///a/b/foo.ri');
      expect(state.activeFile).toBe('/a/b/foo.ri');
      dispose();
    });
  });
});

// ─── editorStore canSave ──────────────────────────────────────────────────────

describe('editorStore canSave', () => {
  it('(a) returns { ok: true, file } when path is in openFiles and not externally changed', () => {
    createRoot((dispose) => {
      const { canSave, openFile } = createEditorStore();
      openFile(file1);
      const result = canSave('bracket.ri');
      expect(result.ok).toBe(true);
      if (result.ok) {
        expect(result.file.path).toBe('bracket.ri');
        expect(result.file.content).toBe(file1.content);
      }
      dispose();
    });
  });

  it('(b) returns { ok: false, reason: "externally-changed" } when path is in openFiles AND externally changed', () => {
    createRoot((dispose) => {
      const { canSave, openFile, markExternallyChanged } = createEditorStore();
      openFile(file1);
      markExternallyChanged('bracket.ri');
      const result = canSave('bracket.ri');
      expect(result.ok).toBe(false);
      if (!result.ok) {
        expect(result.reason).toBe('externally-changed');
      }
      dispose();
    });
  });

  it('(c) returns { ok: false, reason: "not-found" } when path is not in openFiles', () => {
    createRoot((dispose) => {
      const { canSave } = createEditorStore();
      const result = canSave('bracket.ri');
      expect(result.ok).toBe(false);
      if (!result.ok) {
        expect(result.reason).toBe('not-found');
      }
      dispose();
    });
  });

  it('(d) not-found takes precedence over externally-changed when path is absent from openFiles', () => {
    createRoot((dispose) => {
      const { canSave, markExternallyChanged } = createEditorStore();
      // Simulate a pathological state: markExternallyChanged called for a path
      // that is not in openFiles.
      markExternallyChanged('bracket.ri');
      const result = canSave('bracket.ri');
      expect(result.ok).toBe(false);
      if (!result.ok) {
        expect(result.reason).toBe('not-found');
      }
      dispose();
    });
  });

  it('(e) canSave is purely read-only — does not mutate state', () => {
    createRoot((dispose) => {
      const { state, canSave, openFile, markExternallyChanged } = createEditorStore();
      openFile(file1);
      markExternallyChanged('bracket.ri');

      // Capture references BEFORE canSave so identity asserts fire against
      // the pre-call snapshots.  Identity equality is the stronger assertion
      // under Solid's mutable proxy stores: any setState call (even with an
      // equivalent value) produces a new array reference, so `.toBe(refBefore)`
      // directly encodes "no setState was called".  We deliberately use only
      // identity asserts here — content asserts would be strictly weaker (any
      // setState that mutates contents also breaks identity, but identity
      // catches setState calls that happen to preserve contents).
      const ecRefBefore = state.externallyChanged;
      const openFilesRefBefore = state.openFiles;

      canSave('bracket.ri');

      // Reference-identity asserts: confirm canSave performed no setState.
      expect(state.externallyChanged).toBe(ecRefBefore);
      expect(state.openFiles).toBe(openFilesRefBefore);
      dispose();
    });
  });
});

// ─── editorStore missingFiles (step-19) ──────────────────────────────────────

describe('editorStore missingFiles', () => {
  it('(a) initial state.missingFiles is []', () => {
    createRoot((dispose) => {
      const { state } = createEditorStore();
      expect(state.missingFiles).toEqual([]);
      dispose();
    });
  });

  it('(b) markMissing(path) adds the path to missingFiles', () => {
    createRoot((dispose) => {
      const { state, openFile, markMissing } = createEditorStore();
      openFile(file1);
      markMissing('bracket.ri');
      expect(state.missingFiles).toContain('bracket.ri');
      dispose();
    });
  });

  it('(c) markMissing is idempotent — duplicate call does not add a second entry', () => {
    createRoot((dispose) => {
      const { state, openFile, markMissing } = createEditorStore();
      openFile(file1);
      markMissing('bracket.ri');
      markMissing('bracket.ri');
      expect(state.missingFiles.filter((p) => p === 'bracket.ri')).toHaveLength(1);
      dispose();
    });
  });

  it('(c) markMissing idempotency — 1000 calls cause no reactive re-emission', async () => {
    let counter = 0;
    let storeRef!: ReturnType<typeof createEditorStore>;
    let disposeRef!: () => void;

    createRoot((dispose) => {
      disposeRef = dispose;
      storeRef = createEditorStore();
      storeRef.openFile(file1);

      createEffect(() => {
        void storeRef.state.missingFiles.length;
        counter++;
      });
    });

    // Flush the initial effect subscription
    await Promise.resolve();
    expect(counter).toBe(1);

    // First markMissing: new path → real setState → effect fires
    storeRef.markMissing(file1.path);
    await Promise.resolve();
    expect(counter).toBe(2);

    // 1000 more calls on the same already-missing path — no setState
    const refBefore = storeRef.state.missingFiles;
    for (let i = 0; i < 1000; i++) {
      storeRef.markMissing(file1.path);
    }
    await Promise.resolve();
    expect(counter).toBe(2); // no additional emissions
    expect(storeRef.state.missingFiles).toEqual([file1.path]);
    expect(storeRef.state.missingFiles).toBe(refBefore); // same proxy reference

    disposeRef();
  });

  it('(d) clearMissing(path) removes only that path', () => {
    createRoot((dispose) => {
      const { state, openFile, markMissing, clearMissing } = createEditorStore();
      openFile(file1);
      openFile(file2);
      markMissing('bracket.ri');
      markMissing('mount.ri');
      expect(state.missingFiles).toContain('bracket.ri');
      expect(state.missingFiles).toContain('mount.ri');

      clearMissing('bracket.ri');
      expect(state.missingFiles).not.toContain('bracket.ri');
      expect(state.missingFiles).toContain('mount.ri');
      dispose();
    });
  });

  it('(e) closeFile(path) also clears missingFiles for that path', () => {
    createRoot((dispose) => {
      const { state, openFile, closeFile, markMissing } = createEditorStore();
      openFile(file1);
      markMissing('bracket.ri');
      expect(state.missingFiles).toContain('bracket.ri');

      closeFile('bracket.ri');
      expect(state.missingFiles).not.toContain('bracket.ri');
      dispose();
    });
  });

  it('(f) markMissing and markDirty are independent — both can be true simultaneously', () => {
    createRoot((dispose) => {
      const { state, openFile, markMissing, markDirty } = createEditorStore();
      openFile(file1);

      markMissing('bracket.ri');
      markDirty('bracket.ri');
      expect(state.missingFiles).toContain('bracket.ri');
      expect(state.dirtyFiles).toContain('bracket.ri');
      dispose();
    });
  });
});
