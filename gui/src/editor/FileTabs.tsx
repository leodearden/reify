import { For, Show } from 'solid-js';
import type { createEditorStore } from '../stores/editorStore';
import styles from './FileTabs.module.css';

export interface FileTabsProps {
  store: ReturnType<typeof createEditorStore>;
}

export function FileTabs(props: FileTabsProps) {
  function handleKeyDown(e: KeyboardEvent, file: { path: string }) {
    const files = props.store.state.openFiles;
    const idx = files.findIndex((f) => f.path === file.path);
    let nextIdx: number | null = null;

    if (e.key === 'ArrowRight') {
      nextIdx = (idx + 1) % files.length;
    } else if (e.key === 'ArrowLeft') {
      nextIdx = (idx - 1 + files.length) % files.length;
    }

    if (nextIdx !== null) {
      e.preventDefault();
      props.store.setActiveFile(files[nextIdx].path);
      // Focus the newly active tab
      const tabBar = (e.currentTarget as HTMLElement).parentElement;
      const tabs = tabBar?.querySelectorAll('[data-testid="file-tab"]');
      (tabs?.[nextIdx] as HTMLElement)?.focus();
    }
  }

  return (
    <div class={styles.tabBar} data-testid="file-tabs" role="tablist">
      <For each={props.store.state.openFiles}>
        {(file) => {
          const basename = () => file.path.split('/').pop() || file.path;
          const isActive = () => props.store.state.activeFile === file.path;
          const isDirty = () => props.store.state.dirtyFiles.includes(file.path);
          const isExternallyChanged = () => props.store.state.externallyChanged.includes(file.path);
          const isMissing = () => props.store.state.missingFiles.includes(file.path);

          return (
            <div
              class={`${styles.tab}${isActive() ? ` ${styles.active}` : ''}`}
              data-testid="file-tab"
              role="tab"
              aria-selected={isActive() ? 'true' : 'false'}
              tabindex={isActive() ? 0 : -1}
              title={isMissing() ? `${file.path} (missing on disk)` : file.path}
              onClick={() => props.store.setActiveFile(file.path)}
              onKeyDown={(e: KeyboardEvent) => handleKeyDown(e, file)}
            >
              <span>{basename()}</span>
              <Show when={isDirty()}>
                <span class={styles.dirty} data-testid="dirty-indicator" />
              </Show>
              <Show when={isExternallyChanged()}>
                <span class={styles.externallyChanged} data-testid="externally-changed-indicator" />
              </Show>
              <Show when={isMissing()}>
                <span class={styles.missing} data-testid="missing-indicator" />
              </Show>
              <button
                class={styles.closeBtn}
                data-testid="close-tab"
                onClick={(e: MouseEvent) => {
                  e.stopPropagation();
                  if (props.store.state.dirtyFiles.includes(file.path)) {
                    const name = file.path.split('/').pop() || file.path;
                    if (!window.confirm(`"${name}" has unsaved changes. Close anyway?`)) {
                      return;
                    }
                  }
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
