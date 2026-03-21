import { onMount, onCleanup, createEffect } from 'solid-js';
import { EditorState } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { bracketMatching, syntaxHighlighting, defaultHighlightStyle } from '@codemirror/language';
import { reifyLanguage } from './reifyLanguage';
import { updateSource, saveFile } from '../bridge';
import type { createEditorStore } from '../stores/editorStore';
import styles from './Editor.module.css';

export interface EditorProps {
  store: ReturnType<typeof createEditorStore>;
}

export function Editor(props: EditorProps) {
  let containerRef!: HTMLDivElement;
  let view: EditorView | undefined;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  let previousActiveFile: string | null = null;
  let isSwitching = false;

  onMount(() => {
    const activeFile = props.store.state.activeFile;
    previousActiveFile = activeFile;
    const file = props.store.state.openFiles.find((f) => f.path === activeFile);
    const doc = file?.content ?? '';

    const state = EditorState.create({
      doc,
      extensions: [
        reifyLanguage(),
        bracketMatching(),
        syntaxHighlighting(defaultHighlightStyle),
        history(),
        keymap.of([
          {
            key: 'Mod-s',
            run: () => {
              const path = props.store.state.activeFile;
              if (path) {
                saveFile(path)
                  .then(() => props.store.markClean(path))
                  .catch((err: unknown) => console.error('Failed to save file:', err));
              }
              return true;
            },
            preventDefault: true,
          },
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && !isSwitching) {
            const path = props.store.state.activeFile;
            if (path) {
              props.store.markDirty(path);
              clearTimeout(debounceTimer);
              debounceTimer = setTimeout(() => {
                updateSource(path, update.state.doc.toString()).catch((err: unknown) =>
                  console.error('Failed to update source:', err),
                );
              }, 300);
            }
          }
          if (update.selectionSet) {
            const pos = update.state.selection.main.head;
            const line = update.state.doc.lineAt(pos);
            props.store.setCursorPosition(line.number, pos - line.from);
          }
        }),
      ],
    });

    view = new EditorView({ state, parent: containerRef });
  });

  // Watch for active file changes and swap document content
  createEffect(() => {
    const activeFile = props.store.state.activeFile;
    if (!view || activeFile === previousActiveFile) return;
    previousActiveFile = activeFile;

    const file = props.store.state.openFiles.find((f) => f.path === activeFile);
    const newContent = file?.content ?? '';

    isSwitching = true;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: newContent },
      selection: { anchor: 0 },
    });
    isSwitching = false;
  });

  onCleanup(() => {
    clearTimeout(debounceTimer);
    view?.destroy();
  });

  return <div ref={containerRef} class={styles.container} data-testid="editor-container" />;
}
