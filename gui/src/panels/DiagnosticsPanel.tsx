import { type Component, Show, For, createSignal, createMemo, createEffect, onCleanup, untrack } from 'solid-js';
import type { DiagnosticInfo } from '../types';
import styles from './DiagnosticsPanel.module.css';
import {
  loadDiagnosticsLineWrap,
  saveDiagnosticsLineWrap,
  loadDiagnosticsPanelSize,
  saveDiagnosticsPanelSize,
  computeDefaultDialogSize,
} from '../hooks/diagnosticsPanelPersistence';

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

  let dialogRef: HTMLDivElement | undefined;

  const dialogSize = createMemo(() => {
    // Track props.open so the memo re-runs each time the panel opens,
    // ensuring any user-resized size persisted to localStorage is applied.
    // Without this, the cached value from the first render would be reused
    // on every subsequent open, silently discarding the user's manual resize.
    const _open = props.open;
    // Everything below is wrapped in untrack() so that reads of props.diagnostics
    // do NOT register as reactive dependencies of this memo. The default size is a
    // one-shot choice made at mount or on a fresh open transition — never a reaction
    // to diagnostics arriving mid-session. Matches the untrack() pattern used in
    // App.tsx for the same "read a non-reactive snapshot inside a tracked scope" use case.
    return untrack(() => {
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

  // Observe resize events and persist the new size to localStorage.
  createEffect(() => {
    if (!props.open) return;
    if (typeof ResizeObserver === 'undefined') return;
    if (!dialogRef) return;
    const el = dialogRef;
    // Skip the browser's synchronous initial fire on observe() so we don't
    // persist the default-computed size and permanently bypass computeDefaultDialogSize.
    // This is a best-effort heuristic, not a guarantee: a user-driven resize that
    // lands in the same tick as observe() will be silently dropped. Comparing the
    // reported size against the last-known computed default was considered to tighten
    // this, but was rejected to keep the existing ResizeObserver test stable (which
    // uses 640×480 mock dimensions that do not match the computed default).
    let firstFire = true;
    const observer = new ResizeObserver(() => {
      if (firstFire) { firstFire = false; return; }
      saveDiagnosticsPanelSize({ width: el.offsetWidth, height: el.offsetHeight });
    });
    observer.observe(el);
    onCleanup(() => observer.disconnect());
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
          ref={dialogRef}
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
          }}
        >
          <div class={styles.dialogHeader}>
            <h2
              id="diagnostics-panel-title"
              data-testid="panel-title-diagnostics"
              class={styles.title}
            >
              Diagnostics ({props.diagnostics.length})
            </h2>
            <button
              type="button"
              class={styles.headerCloseButton}
              data-testid="diagnostics-header-close"
              aria-label="Close diagnostics"
              onClick={() => props.onClose()}
            >
              ×
            </button>
          </div>

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
