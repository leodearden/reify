import { type Component, Show } from 'solid-js';
import styles from './ReloadPrompt.module.css';

export interface ReloadPromptProps {
  filePaths: string[];
  onReload: () => void;
  onDismiss: () => void;
}

export const ReloadPrompt: Component<ReloadPromptProps> = (props) => {
  function basename(path: string): string {
    return path.split('/').pop() ?? path;
  }

  function message(): string {
    const paths = props.filePaths;
    if (paths.length === 1) {
      return `${basename(paths[0])} changed on disk. Reload?`;
    }
    return `${paths.length} files changed on disk. Reload?`;
  }

  return (
    <Show when={props.filePaths.length > 0}>
      <div data-testid="reload-prompt" class={styles.banner}>
        <span class={styles.message}>
          {message()}
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
    </Show>
  );
};
