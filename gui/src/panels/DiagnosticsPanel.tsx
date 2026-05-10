import { type Component, Show, For, createSignal, createMemo, createEffect, onCleanup } from 'solid-js';
import type { DiagnosticInfo } from '../types';
import styles from './DiagnosticsPanel.module.css';
import {
  loadDiagnosticsLineWrap,
  saveDiagnosticsLineWrap,
  loadDiagnosticsPanelSize,
  computeDefaultDialogSize,
} from '../hooks/useDiagnosticsPanelPersistence';

/** Panel-facing wrapper that extends the wire-format DiagnosticInfo with a
 *  frontend-only source tag. The `source` field is never sent by the Rust
 *  backend; it is added at the App.tsx merge boundary so each row can
 *  display which pipeline produced the entry. */
export interface DiagnosticEntry extends DiagnosticInfo {
  source: 'compile' | 'tessellation';
}

export interface DiagnosticsPanelProps {
  open: boolean;
  diagnostics: DiagnosticEntry[];
  onClose: () => void;
  onNavigate: (d: DiagnosticEntry) => void;
}

export const DiagnosticsPanel: Component<DiagnosticsPanelProps> = (props) => {
  const [lineWrap, setLineWrap] = createSignal(loadDiagnosticsLineWrap() ?? false);

  const dialogSize = createMemo(() => {
    const persisted = loadDiagnosticsPanelSize();
    if (persisted) return persisted;
    const longestChars = props.diagnostics.reduce(
      (max, d) => Math.max(max, d.message?.length ?? 0),
      0
    );
    return computeDefaultDialogSize({
      longestMessageChars: longestChars,
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
    });
  });

  // Attach Escape handler at document level so it fires regardless of which
  // element has focus (the overlay div is unfocused on open, so an
  // element-local onKeyDown would silently no-op for users who press Escape
  // immediately after the panel appears).
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
    // Only close if clicking the overlay itself, not a child
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  }

  function locationLabel(d: DiagnosticInfo): string {
    return `${d.file_path}:${d.line}:${d.column}`;
  }

  function severityClass(severity: string): string {
    switch (severity) {
      case 'Error': return styles.errorBadge;
      case 'Warning': return styles.warningBadge;
      default: return styles.infoBadge;
    }
  }

  function sourceChipClass(source: 'compile' | 'tessellation'): string {
    switch (source) {
      case 'compile': return styles.compileChip;
      case 'tessellation': return styles.tessellationChip;
    }
  }

  return (
    <Show when={props.open}>
      <div
        class={styles.overlay}
        data-testid="diagnostics-panel"
        onClick={handleOverlayClick}
      >
        <div
          data-testid="diagnostics-dialog"
          class={`${styles.dialog}${lineWrap() ? ` ${styles.lineWrapOn}` : ''}`}
          role="dialog"
          aria-modal="true"
          aria-labelledby="diagnostics-panel-title"
          onClick={(e) => e.stopPropagation()}
          style={{
            width: `${dialogSize().width}px`,
            height: `${dialogSize().height}px`,
            resize: 'both',
            overflow: 'auto',
          }}
        >
          <h2
            id="diagnostics-panel-title"
            data-testid="panel-title-diagnostics"
            class={styles.title}
          >
            Diagnostics ({props.diagnostics.length})
          </h2>

          <Show
            when={props.diagnostics.length > 0}
            fallback={
              <span class={styles.emptyState}>No diagnostics</span>
            }
          >
            <div class={styles.list}>
              <For each={props.diagnostics}>
                {(diag) => (
                  <div
                    class={styles.row}
                    data-testid="diagnostic-row"
                    onClick={() => props.onNavigate(diag)}
                    role="button"
                    tabindex="0"
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        props.onNavigate(diag);
                      }
                    }}
                  >
                    <span class={severityClass(diag.severity)}>
                      {diag.severity}
                    </span>
                    <span
                      data-testid="diagnostic-source-chip"
                      class={sourceChipClass(diag.source)}
                    >
                      {diag.source}
                    </span>
                    <span class={styles.location}>{locationLabel(diag)}</span>
                    <span class={styles.message}>{diag.message}</span>
                  </div>
                )}
              </For>
            </div>
          </Show>

          <div class={styles.actions}>
            <label class={styles.wrapLabel}>
              <input
                type="checkbox"
                data-testid="diagnostics-line-wrap-toggle"
                checked={lineWrap()}
                onChange={(e) => {
                  const checked = e.currentTarget.checked;
                  setLineWrap(checked);
                  saveDiagnosticsLineWrap(checked);
                }}
              />
              {' '}Wrap lines
            </label>
            <button class={styles.closeButton} onClick={() => props.onClose()}>
              Close
            </button>
          </div>
        </div>
      </div>
    </Show>
  );
};
