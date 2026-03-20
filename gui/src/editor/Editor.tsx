import { onMount, onCleanup, createEffect } from 'solid-js';
import { EditorState } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { bracketMatching, syntaxHighlighting, defaultHighlightStyle } from '@codemirror/language';
import { reifyLanguage } from './reifyLanguage';
import type { createEditorStore } from '../stores/editorStore';
import styles from './Editor.module.css';

export interface EditorProps {
  store: ReturnType<typeof createEditorStore>;
}

export function Editor(props: EditorProps) {
  let containerRef!: HTMLDivElement;
  let view: EditorView | undefined;

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
        keymap.of([...defaultKeymap, ...historyKeymap]),
      ],
    });

    view = new EditorView({ state, parent: containerRef });
  });

  onCleanup(() => {
    view?.destroy();
  });

  return <div ref={containerRef} class={styles.container} data-testid="editor-container" />;
}
