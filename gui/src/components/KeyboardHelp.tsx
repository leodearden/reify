/**
 * Keyboard shortcut help overlay component.
 */
import { onMount, onCleanup } from 'solid-js';
import { SHORTCUTS } from '../shortcuts';
import styles from './KeyboardHelp.module.css';

export interface KeyboardHelpProps {
  onClose: () => void;
}

export function KeyboardHelp(props: KeyboardHelpProps) {
  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      props.onClose();
    }
  }

  onMount(() => {
    document.addEventListener('keydown', handleKeyDown);
  });

  onCleanup(() => {
    document.removeEventListener('keydown', handleKeyDown);
  });

  return (
    <div class={styles.backdrop} data-testid="keyboard-help" onClick={() => props.onClose()}>
      <div class={styles.card} onClick={(e) => e.stopPropagation()}>
        <h2 class={styles.title}>Keyboard Shortcuts</h2>
        <table class={styles.table}>
          <tbody>
            {SHORTCUTS.filter((s) => s.key).map((s) => (
              <tr>
                <td class={styles.key}><kbd>{s.key}</kbd></td>
                <td class={styles.desc}>{s.description}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <button class={styles.closeBtn} onClick={() => props.onClose()}>Close</button>
      </div>
    </div>
  );
}
