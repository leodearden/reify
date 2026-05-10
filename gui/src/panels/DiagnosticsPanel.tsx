import { type Component, Show, For } from 'solid-js';
import type { DiagnosticInfo } from '../types';
import styles from './DiagnosticsPanel.module.css';

export interface DiagnosticsPanelProps {
  open: boolean;
  diagnostics: DiagnosticInfo[];
  onClose: () => void;
  onNavigate: (d: DiagnosticInfo) => void;
}

export const DiagnosticsPanel: Component<DiagnosticsPanelProps> = (props) => {
  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      props.onClose();
    }
  }

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

  return (
    <Show when={props.open}>
      <div
        class={styles.overlay}
        data-testid="diagnostics-panel"
        onKeyDown={handleKeyDown}
        onClick={handleOverlayClick}
      >
        <div
          class={styles.dialog}
          role="dialog"
          aria-modal="true"
          aria-labelledby="diagnostics-panel-title"
          onClick={(e) => e.stopPropagation()}
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
                    <span class={styles.location}>{locationLabel(diag)}</span>
                    <span class={styles.message}>{diag.message}</span>
                  </div>
                )}
              </For>
            </div>
          </Show>

          <div class={styles.actions}>
            <button class={styles.closeButton} onClick={() => props.onClose()}>
              Close
            </button>
          </div>
        </div>
      </div>
    </Show>
  );
};
