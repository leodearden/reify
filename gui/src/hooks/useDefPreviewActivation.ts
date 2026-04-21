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

  createEffect(() => {
    // Read cursor reactively so the effect re-runs on every cursor change
    const pos = editorStore.state.cursorPosition;

    // Clear any pending debounce timer
    if (timerId !== null) {
      clearTimeout(timerId);
      timerId = null;
    }

    // Skip scheduling if there is no cursor position
    if (pos === null) return;

    const { line, column } = pos;

    timerId = setTimeout(async () => {
      timerId = null;
      const result = await getContainingDefinition(line, column);

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
