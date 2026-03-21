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

  return (
    <div data-testid="file-browser" class={styles.browser}>
      <Show
        when={props.files.length > 0}
        fallback={<div class={styles.empty}>No files</div>}
      >
        <For each={props.files}>
          {(file) => (
            <div
              data-testid={`file-item-${file.path}`}
              data-active={file.path === props.activeFile ? 'true' : undefined}
              class={styles.item}
              onClick={() => props.onFileClick(file.path)}
            >
              {basename(file.path)}
            </div>
          )}
        </For>
      </Show>
    </div>
  );
};
