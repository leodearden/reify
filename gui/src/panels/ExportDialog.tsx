import { type Component, Show, createSignal, onMount } from 'solid-js';
import type { ExportFormat } from '../types';
import styles from './ExportDialog.module.css';

export interface ExportDialogProps {
  open: boolean;
  exporting: boolean;
  onExport: (format: ExportFormat) => void;
  onClose: () => void;
}

const FOCUSABLE_SELECTOR = 'button:not([disabled]), select:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex="-1"])';

export const ExportDialog: Component<ExportDialogProps> = (props) => {
  const [format, setFormat] = createSignal<ExportFormat>('step');

  function setupFocusTrap(dialogEl: HTMLDivElement) {
    // Auto-focus the first focusable element (deferred to ensure DOM is ready)
    queueMicrotask(() => {
      const focusable = dialogEl.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR);
      if (focusable.length > 0) {
        focusable[0].focus();
      }
    });
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !props.exporting) {
      props.onClose();
      return;
    }

    if (e.key === 'Tab') {
      const overlay = e.currentTarget as HTMLElement;
      const dialog = overlay.querySelector('[role="dialog"]');
      if (!dialog) return;

      const focusable = dialog.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR);
      if (focusable.length === 0) return;

      const first = focusable[0];
      const last = focusable[focusable.length - 1];

      if (e.shiftKey) {
        if (document.activeElement === first) {
          e.preventDefault();
          last.focus();
        }
      } else {
        if (document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    }
  }

  return (
    <Show when={props.open}>
      <div
        class={styles.overlay}
        data-testid="export-dialog"
        onKeyDown={handleKeyDown}
        onClick={() => {
          if (!props.exporting) {
            props.onClose();
          }
        }}
      >
        <div
          ref={(el) => setupFocusTrap(el)}
          class={styles.dialog}
          role="dialog"
          aria-modal="true"
          aria-labelledby="export-dialog-title"
          onClick={(e) => e.stopPropagation()}
        >
          <h2 id="export-dialog-title" class={styles.title}>Export Geometry</h2>

          <Show when={props.exporting}>
            <div class={styles.progress} data-testid="export-progress">
              Exporting...
            </div>
          </Show>

          <div class={styles.field}>
            <label class={styles.label} for="export-format-select">Format</label>
            <select
              id="export-format-select"
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
