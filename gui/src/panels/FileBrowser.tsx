import { type Component, For, Show } from 'solid-js';
import type { FileData } from '../types';
import styles from './FileBrowser.module.css';

export interface FileBrowserProps {
  files: FileData[];
  activeFile: string | null;
  onFileClick: (path: string) => void;
}

export const FileBrowser: Component<FileBrowserProps> = (props) => {
  function basename(path: string): string {
    return path.split('/').pop() ?? path;
  }

  function handleKeyDown(e: KeyboardEvent, file: FileData) {
    const files = props.files;
    const idx = files.findIndex((f) => f.path === file.path);
    let targetIdx: number | null = null;

    if (e.key === 'ArrowDown') {
      targetIdx = idx + 1 < files.length ? idx + 1 : null;
    } else if (e.key === 'ArrowUp') {
      targetIdx = idx - 1 >= 0 ? idx - 1 : null;
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      props.onFileClick(file.path);
      return;
    }

    if (targetIdx !== null) {
      e.preventDefault();
      props.onFileClick(files[targetIdx].path);
      // Focus the target item
      const container = (e.currentTarget as HTMLElement).parentElement;
      const items = container?.querySelectorAll('[role="option"]');
      (items?.[targetIdx] as HTMLElement)?.focus();
    }
  }

  return (
    <div data-testid="file-browser" class={styles.browser} role="listbox" aria-label="File browser">
      <Show
        when={props.files.length > 0}
        fallback={<div class={styles.empty}>No files</div>}
      >
        <For each={props.files}>
          {(file) => {
            const isActive = () => file.path === props.activeFile;
            return (
              <div
                data-testid={`file-item-${file.path}`}
                data-active={isActive() ? 'true' : undefined}
                class={styles.item}
                role="option"
                aria-selected={isActive() ? 'true' : 'false'}
                tabindex={isActive() ? 0 : -1}
                title={file.path}
                onClick={() => props.onFileClick(file.path)}
                onKeyDown={(e: KeyboardEvent) => handleKeyDown(e, file)}
              >
                {basename(file.path)}
              </div>
            );
          }}
        </For>
      </Show>
    </div>
  );
};
