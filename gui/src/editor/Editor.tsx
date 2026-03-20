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

  onMount(() => {
    const activeFile = props.store.state.activeFile;
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
                saveFile(path);
                props.store.markClean(path);
              }
              return true;
            },
            preventDefault: true,
          },
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            const path = props.store.state.activeFile;
            if (path) {
              props.store.markDirty(path);
              clearTimeout(debounceTimer);
              debounceTimer = setTimeout(() => {
                updateSource(path, update.state.doc.toString());
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

  onCleanup(() => {
    clearTimeout(debounceTimer);
    view?.destroy();
  });

  return <div ref={containerRef} class={styles.container} data-testid="editor-container" />;
}
