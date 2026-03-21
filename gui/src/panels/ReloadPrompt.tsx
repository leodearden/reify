import { type Component, Show } from 'solid-js';
import styles from './ReloadPrompt.module.css';

export interface ReloadPromptProps {
  filePath: string | null;
  onReload: () => void;
  onDismiss: () => void;
}

export const ReloadPrompt: Component<ReloadPromptProps> = (props) => {
  function basename(path: string): string {
    return path.split('/').pop() ?? path;
  }

  return (
    <Show when={props.filePath}>
      {(path) => (
        <div data-testid="reload-prompt" class={styles.banner}>
          <span class={styles.message}>
            {basename(path())} changed on disk. Reload?
          </span>
          <div class={styles.actions}>
            <button
              class={`${styles.button} ${styles.reload}`}
              onClick={() => props.onReload()}
            >
              Reload
            </button>
            <button
              class={`${styles.button} ${styles.dismiss}`}
              onClick={() => props.onDismiss()}
            >
              Dismiss
            </button>
          </div>
        </div>
      )}
    </Show>
  );
};
