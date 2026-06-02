import { createEffect, onCleanup, untrack } from 'solid-js';

// ── createEditorHoverSync types ───────────────────────────────────────────────

export interface EditorHoverSyncOptions {
  editorStore: { state: { cursorPosition: { line: number; column: number } | null } };
  getEntityAtSourceLocation: (line: number, col: number) => Promise<string | null>;
  hoverEntity: (path: string | null) => void;
  debounceMs?: number;
}

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
    // Snapshot the current selection synchronously at cursor-change time.
    // untrack() prevents this read from adding selectionStore to the effect's
    // dependency set — we want the effect to re-run only on cursor changes, not
    // on viewport-driven selection mutations.  The snapshot is closed over by
    // the setTimeout callback so the cross-input race guard treats the entire
    // request lifetime — debounce window AND in-flight bridge call — as the
    // protected window during which any non-editor mutation wins.
    // Refreshed on every cursor-change re-run, so a viewport click followed by
    // another cursor move resets the snapshot and lets the editor win.
    const selectionAtCursorChange = untrack(() => selectionStore.state.selectedEntity);

    timerId = setTimeout(async () => {
      timerId = null;

      // Early-exit: if selection changed during the debounce window, the bridge
      // result will be discarded anyway by the cross-input race guard — skip the
      // round-trip entirely.
      if (untrack(() => selectionStore.state.selectedEntity) !== selectionAtCursorChange) return;

      // Capture (do not increment) the current token before the await
      const token = latestRequestToken;
      const result = await getEntityAtSourceLocation(line, column);

      // Discard stale results: a newer cursor-move fired while this was in flight
      if (token !== latestRequestToken) return;

      // Null result → keep existing selection unchanged (no bounce on whitespace/comments)
      if (result === null) return;

      // Cross-input race guard: if selection was changed by a non-editor source
      // (e.g. a viewport click) at any point since the cursor moved — including
      // during the debounce window before the bridge call started — discard this
      // result to avoid overwriting the externally-set selection.
      if (selectionStore.state.selectedEntity !== selectionAtCursorChange) return;

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

// ── createEditorHoverSync ─────────────────────────────────────────────────────

/**
 * createEditorHoverSync
 *
 * Watches `editorStore.state.cursorPosition`, debounces by `debounceMs`
 * (default 200ms), resolves `getEntityAtSourceLocation(line, col)`, then calls
 * `hoverEntity(result | null)`:
 * - null cursor pos → hoverEntity(null) immediately (clear on hover-off)
 * - null resolution → hoverEntity(null) (cursor on whitespace/comment)
 * - non-null resolution → hoverEntity(entity)
 *
 * Uses a latestRequestToken race-guard to discard stale async results.
 *
 * Cleanup behaviour: cancels any pending debounce timer only. Does NOT call
 * hoverEntity(null) on disposal — the hook is mounted at App root so teardown
 * coincides with app exit; there is no live consumer left to clear. If you
 * reuse this hook in a shorter-lived scope you should call hoverEntity(null)
 * yourself on unmount.
 *
 * Cross-source race (intentional): mouse-originated hover writes
 * (DualViewport.onHover / DesignTree.onHover → selectionStore.hoverEntity)
 * and editor-resolved hover writes both target hoverEntity directly; the last
 * writer wins. Hover is transient and low-stakes — the full cross-input
 * snapshot guard from createEditorSelectionSync is not warranted here.
 */
export function createEditorHoverSync(opts: EditorHoverSyncOptions): void {
  const {
    editorStore,
    getEntityAtSourceLocation,
    hoverEntity,
    debounceMs = 200,
  } = opts;

  let timerId: ReturnType<typeof setTimeout> | null = null;
  let latestRequestToken = 0;

  createEffect(() => {
    const pos = editorStore.state.cursorPosition;

    if (timerId !== null) {
      clearTimeout(timerId);
      timerId = null;
    }

    ++latestRequestToken;

    if (pos === null) {
      hoverEntity(null);
      return;
    }

    const { line, column } = pos;

    timerId = setTimeout(async () => {
      timerId = null;

      const token = latestRequestToken;
      const result = await getEntityAtSourceLocation(line, column);

      if (token !== latestRequestToken) return;

      hoverEntity(result);
    }, debounceMs);
  });

  onCleanup(() => {
    if (timerId !== null) {
      clearTimeout(timerId);
      timerId = null;
    }
  });
}
