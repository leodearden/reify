import { type Component, Show, createSignal } from 'solid-js';
import type { ExportFormat } from '../types';
import styles from './ExportDialog.module.css';

export interface ExportDialogProps {
  open: boolean;
  exporting: boolean;
  onExport: (format: ExportFormat) => void;
  onClose: () => void;
}

export const ExportDialog: Component<ExportDialogProps> = (props) => {
  const [format, setFormat] = createSignal<ExportFormat>('step');

  return (
    <Show when={props.open}>
      <div class={styles.overlay} data-testid="export-dialog">
        <div class={styles.dialog} role="dialog" aria-modal="true" aria-labelledby="export-dialog-title">
          <h2 id="export-dialog-title" class={styles.title}>Export Geometry</h2>

          <Show when={props.exporting}>
            <div class={styles.progress} data-testid="export-progress">
              Exporting...
            </div>
          </Show>

          <div class={styles.field}>
            <label class={styles.label}>Format</label>
            <select
              class={styles.select}
              value={format()}
              disabled={props.exporting}
              onChange={(e) => setFormat(e.currentTarget.value as ExportFormat)}
            >
              <option value="step">STEP</option>
              <option value="stl">STL</option>
              <option value="3mf">3MF</option>
            </select>
          </div>

          <div class={styles.actions}>
            <button
              class={`${styles.button} ${styles.secondary}`}
              disabled={props.exporting}
              onClick={() => props.onClose()}
            >
              Cancel
            </button>
            <button
              class={`${styles.button} ${styles.primary}`}
              disabled={props.exporting}
              onClick={() => props.onExport(format())}
            >
              Export
            </button>
          </div>
        </div>
      </div>
    </Show>
  );
};
