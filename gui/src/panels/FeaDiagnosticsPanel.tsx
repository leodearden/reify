/**
 * FeaDiagnosticsPanel — sidebar panel for FEA failure-mode diagnostics (#2966, step-14).
 *
 * Renders rows from feaDiagnosticRows(), one per FeaDiagnosticInfo.
 * Each row is a role=button with click + Enter/Space keyboard handler that
 * invokes onFocusDiagnostic with that diagnostic (camera focus / fitToView).
 *
 * Modeled on DiagnosticsPanel.tsx (For/Show + role=button click+keydown pattern).
 * Exported from panels/index.ts alongside its props type.
 */

import { type Component, Show, For } from 'solid-js';
import type { FeaDiagnosticInfo } from '../types';
import { feaDiagnosticRows } from './feaDiagnosticsView';
import styles from './FeaDiagnosticsPanel.module.css';

export interface FeaDiagnosticsPanelProps {
  /** FEA diagnostic payloads from engineStore.state.feaDiagnostics. */
  diagnostics: FeaDiagnosticInfo[];
  /** Called when the user clicks / activates a row (camera focus). */
  onFocusDiagnostic: (d: FeaDiagnosticInfo) => void;
}

export const FeaDiagnosticsPanel: Component<FeaDiagnosticsPanelProps> = (props) => {
  const rows = () => feaDiagnosticRows(props.diagnostics);

  return (
    <div
      data-testid="fea-diagnostics-panel"
      class={styles.panel}
    >
      <span class={styles.header}>
        FEA Diagnostics ({props.diagnostics.length})
      </span>

      <Show
        when={rows().length > 0}
        fallback={
          <span class={styles.emptyState}>No FEA diagnostics</span>
        }
      >
        <div class={styles.list}>
          <For each={rows()}>
            {(row, i) => {
              const diag = () => props.diagnostics[i()];
              return (
                <div
                  data-testid="fea-diagnostic-row"
                  class={styles.row}
                  role="button"
                  tabindex="0"
                  onClick={() => props.onFocusDiagnostic(diag())}
                  onKeyDown={(e: KeyboardEvent) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      props.onFocusDiagnostic(diag());
                    }
                  }}
                >
                  <span class={styles.rowLabel}>{row.label}</span>
                  <span class={styles.rowDetail}>{row.detail}</span>
                </div>
              );
            }}
          </For>
        </div>
      </Show>
    </div>
  );
};
