import { type Component, Show, For, createEffect, onCleanup } from 'solid-js';
import type { ReferenceResult } from '../editor/references';
import styles from './FindUsesPanel.module.css';

/**
 * Find-uses panel — a focused clone of DiagnosticsPanel for the
 * textDocument/references provider (task 4202 β).
 *
 * Lists every in-scope reference (declaration ∪ uses) returned for the symbol
 * under the cursor when the user presses Shift+F12. Clicking a row navigates to
 * that occurrence via the App-level onNavigate handler (which reuses the
 * diagnostics setScrollToLocation path, so the cursor moves and a nav-history
 * entry is recorded — no new plumbing).
 */
export interface FindUsesPanelProps {
  open: boolean;
  results: ReferenceResult[];
  onClose: () => void;
  onNavigate: (r: ReferenceResult) => void;
}

export const FindUsesPanel: Component<FindUsesPanelProps> = (props) => {
  // Document-level Escape handler (mirrors DiagnosticsPanel): fires regardless
  // of which element has focus, since the overlay is unfocused on open.
  createEffect(() => {
    if (!props.open) return;
    function handleDocumentKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        props.onClose();
      }
    }
    document.addEventListener('keydown', handleDocumentKeyDown);
    onCleanup(() => document.removeEventListener('keydown', handleDocumentKeyDown));
  });

  function handleOverlayClick(e: MouseEvent) {
    // Only close if clicking the overlay itself, not a child.
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  }

  // LSP positions are 0-based; the editor's SourceLocation / cursor reporting
  // is 1-based, so the displayed label is line+1 : character+1.
  function locationLabel(r: ReferenceResult): string {
    return `${r.line + 1}:${r.character + 1}`;
  }

  return (
    <Show when={props.open}>
      <div
        class={styles.overlay}
        data-testid="find-uses-panel"
        onClick={handleOverlayClick}
      >
        <div
          data-testid="find-uses-dialog"
          class={styles.dialog}
          role="dialog"
          aria-modal="true"
          aria-labelledby="find-uses-panel-title"
          onClick={(e) => e.stopPropagation()}
        >
          <div class={styles.dialogHeader}>
            <h2
              id="find-uses-panel-title"
              data-testid="panel-title-find-uses"
              class={styles.title}
            >
              Find uses ({props.results.length})
            </h2>
            <button
              type="button"
              class={styles.headerCloseButton}
              data-testid="find-uses-header-close"
              aria-label="Close find uses"
              onClick={() => props.onClose()}
            >
              ×
            </button>
          </div>

          <Show
            when={props.results.length > 0}
            fallback={<span class={styles.emptyState}>No uses found</span>}
          >
            <div class={styles.list}>
              <For each={props.results}>
                {(result) => (
                  <div
                    class={styles.row}
                    data-testid="find-use-row"
                    role="button"
                    tabindex="0"
                    onClick={() => props.onNavigate(result)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        props.onNavigate(result);
                      }
                    }}
                  >
                    <span class={styles.location}>{locationLabel(result)}</span>
                    <Show when={result.preview}>
                      <span class={styles.preview}>{result.preview}</span>
                    </Show>
                  </div>
                )}
              </For>
            </div>
          </Show>

          <div class={styles.actions}>
            <button
              type="button"
              class={styles.closeButton}
              data-testid="find-uses-close"
              onClick={() => props.onClose()}
            >
              Close
            </button>
          </div>
        </div>
      </div>
    </Show>
  );
};
