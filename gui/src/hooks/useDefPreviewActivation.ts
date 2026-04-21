import { createSignal, createEffect, onCleanup } from 'solid-js';
import type { DefInfo, GuiState } from '../types';

// ── Dependency types ─────────────────────────────────────────────────────────

interface EditorStoreLike {
  state: { cursorPosition: { line: number; column: number } | null };
}

interface ViewportStoreLike {
  setDefPath: (viewportId: string, defPath: string | null) => boolean;
}

interface DefPreviewStoreLike {
  state: { defName: string | null };
  loadPreview: (defName: string, fetch: (name: string) => Promise<GuiState>) => Promise<void>;
  clearPreview: () => void;
}

export interface DefPreviewActivationOptions {
  editorStore: EditorStoreLike;
  viewportStore: ViewportStoreLike;
  defPreviewStore: DefPreviewStoreLike;
  getContainingDefinition: (line: number, col: number) => Promise<DefInfo | null>;
  getDefPreview: (defName: string) => Promise<GuiState>;
  debounceMs?: number;
}

export interface DefPreviewActivation {
  defInfo: () => DefInfo | null;
  defPreviewActive: () => boolean;
}

// ── Hook implementation ───────────────────────────────────────────────────────

/**
 * createDefPreviewActivation
 *
 * Watches `editorStore.state.cursorPosition`, debounces by `debounceMs` (default 200ms),
 * then calls `getContainingDefinition(line, col)`:
 * - On DefInfo result: calls `viewportStore.setDefPath('def-preview', name)` and
 *   `defPreviewStore.loadPreview(name, getDefPreview)`.
 * - On null result: calls `viewportStore.setDefPath('def-preview', null)` and
 *   `defPreviewStore.clearPreview()`.
 *
 * Dependency-injected so the hook is unit-testable without Tauri or a real DOM.
 */
export function createDefPreviewActivation(opts: DefPreviewActivationOptions): DefPreviewActivation {
  const {
    editorStore,
    viewportStore,
    defPreviewStore,
    getContainingDefinition,
    getDefPreview,
    debounceMs = 200,
  } = opts;

  const [defInfo, setDefInfo] = createSignal<DefInfo | null>(null);
  const defPreviewActive = () => defInfo() !== null;

  let timerId: ReturnType<typeof setTimeout> | null = null;

  /**
   * Monotonically-increasing request token.
   *
   * Bumped synchronously in the createEffect body on every cursor change
   * (after `clearTimeout`, before `setTimeout`). The debounce callback captures
   * (but does not increment) the current value before awaiting
   * `getContainingDefinition`: `const token = latestRequestToken`. After the
   * await, if `token !== latestRequestToken` the result is stale and is silently
   * discarded.
   *
   * Moving the bump to the effect body closes a race window: if the cursor moves
   * after a debounce timer fires (T1) but before the next timer fires (T2), T1's
   * in-flight request is immediately invalidated even though T2 has not yet
   * captured its token.
   */
  let latestRequestToken = 0;

  createEffect(() => {
    // Read cursor reactively so the effect re-runs on every cursor change
    const pos = editorStore.state.cursorPosition;

    // Clear any pending debounce timer
    if (timerId !== null) {
      clearTimeout(timerId);
      timerId = null;
    }

    // Bump the request token synchronously on every cursor change so any
    // in-flight getContainingDefinition call whose captured token is older
    // is immediately invalidated — even if the next debounce timer hasn't fired yet.
    ++latestRequestToken;

    // Skip scheduling if there is no cursor position
    if (pos === null) return;

    const { line, column } = pos;

    timerId = setTimeout(async () => {
      timerId = null;
      // Capture (do not increment) the current token before the await
      const token = latestRequestToken;
      const result = await getContainingDefinition(line, column);

      // Discard stale results: a newer request fired while this one was in flight
      if (token !== latestRequestToken) return;

      if (result !== null) {
        setDefInfo(result);
        viewportStore.setDefPath('def-preview', result.name);
        void defPreviewStore.loadPreview(result.name, getDefPreview);
      } else {
        setDefInfo(null);
        viewportStore.setDefPath('def-preview', null);
        defPreviewStore.clearPreview();
      }
    }, debounceMs);
  });

  onCleanup(() => {
    if (timerId !== null) {
      clearTimeout(timerId);
      timerId = null;
    }
  });

  return { defInfo, defPreviewActive };
}
