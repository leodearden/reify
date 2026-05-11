import { createEffect, onCleanup } from 'solid-js';

// ── Dependency types ─────────────────────────────────────────────────────────

interface EditorStoreLike {
  state: { cursorPosition: { line: number; column: number } | null };
}

interface SelectionStoreLike {
  state: { selectedEntity: string | null };
}

export interface EditorSelectionSyncOptions {
  editorStore: EditorStoreLike;
  selectionStore: SelectionStoreLike;
  getEntityAtSourceLocation: (line: number, col: number) => Promise<string | null>;
  selectEntity: (entityPath: string) => void;
  flyToEntity?: (entityPath: string) => void;
  debounceMs?: number;
}

// ── Hook implementation ───────────────────────────────────────────────────────

/**
 * createEditorSelectionSync
 *
 * Watches `editorStore.state.cursorPosition`, debounces by `debounceMs`
 * (default 200ms), then calls `getEntityAtSourceLocation(line, col)`:
 * - If the returned entity is non-null AND differs from the current
 *   `selectionStore.state.selectedEntity`: calls `selectEntity(entity)` and
 *   `flyToEntity?.(entity)`.
 * - If the returned entity is null OR equal to the current selection: no-op
 *   (preserves existing selection, prevents feedback-loop bouncing).
 *
 * Uses the same `latestRequestToken` race-guard as `createDefPreviewActivation`
 * to discard stale results when a newer request has fired.
 *
 * Dependency-injected so the hook is unit-testable without Tauri or a real DOM.
 */
export function createEditorSelectionSync(opts: EditorSelectionSyncOptions): void {
  const {
    editorStore,
    selectionStore,
    getEntityAtSourceLocation,
    selectEntity,
    flyToEntity,
    debounceMs = 200,
  } = opts;

  let timerId: ReturnType<typeof setTimeout> | null = null;

  /**
   * Monotonically-increasing request token. Bumped synchronously in the
   * createEffect body on every cursor change so any in-flight request whose
   * captured token is older is immediately invalidated — even if the next
   * debounce timer hasn't fired yet.
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

    // Bump the request token synchronously on every cursor change
    ++latestRequestToken;

    // Skip scheduling if there is no cursor position
    if (pos === null) return;

    const { line, column } = pos;

    timerId = setTimeout(async () => {
      timerId = null;
      // Capture (do not increment) the current token before the await
      const token = latestRequestToken;
      // Snapshot the current selection BEFORE the await so we can detect
      // cross-input mutations (e.g. viewport click) that happen while the
      // bridge call is in flight.
      const selectionBeforeAwait = selectionStore.state.selectedEntity;
      const result = await getEntityAtSourceLocation(line, column);

      // Discard stale results: a newer cursor-move fired while this was in flight
      if (token !== latestRequestToken) return;

      // Null result → keep existing selection unchanged (no bounce on whitespace/comments)
      if (result === null) return;

      // Cross-input race guard: if selection was changed by a non-editor source
      // (e.g. a viewport click) while the bridge call was in flight, discard
      // this result to avoid overwriting the externally-set selection.
      if (selectionStore.state.selectedEntity !== selectionBeforeAwait) return;

      // Equality-check guard: skip if the entity is already selected (prevents
      // viewport-click → editor-scroll → cursor-move → re-select bounce)
      if (result === selectionStore.state.selectedEntity) return;

      selectEntity(result);
      flyToEntity?.(result);
    }, debounceMs);
  });

  onCleanup(() => {
    if (timerId !== null) {
      clearTimeout(timerId);
      timerId = null;
    }
  });
}
