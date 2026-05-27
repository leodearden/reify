import { createStore } from 'solid-js/store';
import { canonicalizeKey } from '../utils/pathUtils';
import type { FileData } from '../types';

/**
 * Reason codes for a blocked save attempt.
 *
 * Defined here (in the data layer) so that `editorStore` is self-contained and
 * does not depend on the UI-message module.  `messages.ts` re-exports this type
 * so consumers can import it from either location.
 */
export type SaveBlockedReason = 'externally-changed' | 'not-found';

/** Discriminated result from {@link createEditorStore}'s `canSave`. */
export type CanSaveResult =
  | { ok: true; file: FileData }
  | { ok: false; reason: SaveBlockedReason };

export interface EditorState {
  openFiles: FileData[];
  activeFile: string | null;
  dirtyFiles: string[];
  externallyChanged: string[];
  /** Paths of open files whose backing disk entry has been deleted. */
  missingFiles: string[];
  cursorPosition: { line: number; column: number } | null;
}

export function createEditorStore() {
  const [state, setState] = createStore<EditorState>({
    openFiles: [],
    activeFile: null,
    dirtyFiles: [],
    externallyChanged: [],
    missingFiles: [],
    cursorPosition: null,
  });

  function openFile(file: FileData) {
    const key = canonicalizeKey(file.path);
    const canonical: FileData = { ...file, path: key };
    const exists = state.openFiles.some((f) => f.path === key);
    if (!exists) {
      setState('openFiles', (files) => [...files, canonical]);
    } else {
      updateFileContent(key, file.content);
    }
    setState('activeFile', key);
  }

  function updateFileContent(path: string, content: string) {
    const key = canonicalizeKey(path);
    setState(
      'openFiles',
      (f) => f.path === key,
      'content',
      content,
    );
  }

  function closeFile(path: string) {
    const key = canonicalizeKey(path);
    const closedIndex = state.openFiles.findIndex((f) => f.path === key);
    const remaining = state.openFiles.filter((f) => f.path !== key);
    setState('openFiles', remaining);
    setState('dirtyFiles', (dirty) => dirty.filter((p) => p !== key));
    setState('externallyChanged', (ec) => ec.filter((p) => p !== key));
    setState('missingFiles', (mf) => mf.filter((p) => p !== key));
    if (state.activeFile === key) {
      const next = remaining[closedIndex] ?? remaining[closedIndex - 1] ?? null;
      setState('activeFile', next ? next.path : null);
    }
  }

  function setActiveFile(path: string) {
    setState('activeFile', canonicalizeKey(path));
  }

  function markDirty(path: string) {
    const key = canonicalizeKey(path);
    if (!state.dirtyFiles.includes(key)) {
      setState('dirtyFiles', (dirty) => [...dirty, key]);
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
    const key = canonicalizeKey(path);
    setState('dirtyFiles', (dirty) => dirty.filter((p) => p !== key));
    setState('externallyChanged', (ec) => ec.filter((p) => p !== key));
  }

  function markExternallyChanged(path: string) {
    const key = canonicalizeKey(path);
    if (!state.externallyChanged.includes(key)) {
      setState('externallyChanged', (ec) => [...ec, key]);
    }
  }

  function clearExternallyChanged(path: string) {
    const key = canonicalizeKey(path);
    setState('externallyChanged', (ec) => ec.filter((p) => p !== key));
  }

  function clearAllExternallyChanged() {
    if (state.externallyChanged.length === 0) return;
    setState('externallyChanged', []);
  }

  function markMissing(path: string) {
    const key = canonicalizeKey(path);
    if (!state.missingFiles.includes(key)) {
      setState('missingFiles', (mf) => [...mf, key]);
    }
  }

  function clearMissing(path: string) {
    const key = canonicalizeKey(path);
    setState('missingFiles', (mf) => mf.filter((p) => p !== key));
  }

  /**
   * Read-only policy helper: determines whether a save attempt for `path`
   * should proceed.
   *
   * - If the path is not in `openFiles`, returns `{ ok: false, reason: 'not-found' }`.
   * - If the path is externally changed, returns `{ ok: false, reason: 'externally-changed' }`.
   * - Otherwise returns `{ ok: true, file }` with the resolved FileData so
   *   callers don't need a redundant `find` + non-null assertion.
   *
   * `not-found` takes precedence over `externally-changed`: a path absent from
   * openFiles cannot meaningfully be "externally changed" from the editor's
   * perspective.
   */
  function canSave(path: string): CanSaveResult {
    const key = canonicalizeKey(path);
    const file = state.openFiles.find((f) => f.path === key);
    if (!file) return { ok: false, reason: 'not-found' };
    if (state.externallyChanged.includes(key)) return { ok: false, reason: 'externally-changed' };
    return { ok: true, file };
  }

  function setCursorPosition(lineOrNull: number | null, column?: number) {
    if (lineOrNull === null) {
      setState('cursorPosition', null);
    } else {
      setState('cursorPosition', { line: lineOrNull, column: column ?? 0 });
    }
  }

  return { state, openFile, updateFileContent, closeFile, setActiveFile, markDirty, markClean, markExternallyChanged, clearExternallyChanged, clearAllExternallyChanged, markMissing, clearMissing, setCursorPosition, canSave };
}
