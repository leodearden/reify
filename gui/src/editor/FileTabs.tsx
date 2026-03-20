import { For, Show } from 'solid-js';
import type { createEditorStore } from '../stores/editorStore';
import styles from './FileTabs.module.css';

export interface FileTabsProps {
  store: ReturnType<typeof createEditorStore>;
}

export function FileTabs(props: FileTabsProps) {
  return (
    <div class={styles.tabBar} data-testid="file-tabs">
      <For each={props.store.state.openFiles}>
        {(file) => {
          const basename = () => file.path.split('/').pop() || file.path;
          const isActive = () => props.store.state.activeFile === file.path;
          const isDirty = () => props.store.state.dirtyFiles.includes(file.path);

          return (
            <div
              class={`${styles.tab}${isActive() ? ` ${styles.active}` : ''}`}
              data-testid="file-tab"
              role="tab"
              aria-selected={isActive() ? 'true' : 'false'}
              onClick={() => props.store.setActiveFile(file.path)}
            >
              <span>{basename()}</span>
              <Show when={isDirty()}>
                <span class={styles.dirty} data-testid="dirty-indicator" />
              </Show>
              <button
                class={styles.closeBtn}
                data-testid="close-tab"
                onClick={(e: MouseEvent) => {
                  e.stopPropagation();
                  props.store.closeFile(file.path);
                }}
              >
                ×
              </button>
            </div>
          );
        }}
      </For>
    </div>
  );
}
