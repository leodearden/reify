import { onMount, onCleanup, createEffect } from 'solid-js';
import { EditorState } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { bracketMatching, syntaxHighlighting, defaultHighlightStyle } from '@codemirror/language';
import { autocompletion } from '@codemirror/autocomplete';
import { linter, setDiagnostics, type Diagnostic } from '@codemirror/lint';
import { reifyLanguage } from './reifyLanguage';
import { updateSource, saveFile } from '../bridge';
import { createLspClient } from './lspClient';
import { reifyCompletionSource } from './completions';
import { createDiagnosticsListener, lspDiagnosticToCodeMirror } from './diagnostics';
import { reifyHoverTooltip } from './hover';
import { reifyGotoDefinition } from './gotoDefinition';
import type { createEditorStore } from '../stores/editorStore';
import styles from './Editor.module.css';

export interface EditorProps {
  store: ReturnType<typeof createEditorStore>;
}

export function Editor(props: EditorProps) {
  let containerRef!: HTMLDivElement;
  let view: EditorView | undefined;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  let lspDebounceTimer: ReturnType<typeof setTimeout> | undefined;
  let previousActiveFile: string | null = null;
  let isSwitching = false;
  let lspVersion = 1;
  let unlistenDiagnostics: (() => void) | undefined;

  // Create LSP client for communicating with the in-process LSP server
  const lspClient = createLspClient();

  /** Convert active file path to a file:// URI for LSP. */
  function pathToUri(path: string): string {
    if (path.startsWith('file://')) return path;
    return `file://${path.startsWith('/') ? '' : '/'}${path}`;
  }

  onMount(() => {
    const activeFile = props.store.state.activeFile;
    previousActiveFile = activeFile;
    const file = props.store.state.openFiles.find((f) => f.path === activeFile);
    const doc = file?.content ?? '';
    const uri = activeFile ? pathToUri(activeFile) : 'file:///untitled.ri';

    const state = EditorState.create({
      doc,
      extensions: [
        reifyLanguage(),
        bracketMatching(),
        syntaxHighlighting(defaultHighlightStyle),
        history(),
        // LSP-powered completions
        autocompletion({ override: [reifyCompletionSource(uri)] }),
        // LSP-powered hover tooltips
        reifyHoverTooltip(uri),
        // LSP-powered go-to-definition (Ctrl+Click)
        reifyGotoDefinition(uri),
        // Diagnostic linter (diagnostics are pushed from LSP via Tauri events)
        linter(() => [] as Diagnostic[]),
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

              // Send didChange to LSP (debounced)
              clearTimeout(lspDebounceTimer);
              lspDebounceTimer = setTimeout(() => {
                lspVersion++;
                lspClient
                  .didChange(pathToUri(path), update.state.doc.toString(), lspVersion)
                  .catch((err: unknown) => console.error('LSP didChange error:', err));
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

    // Initialize LSP and open the document
    lspClient
      .initialize()
      .then(() => {
        if (activeFile) {
          return lspClient.didOpen(uri, doc, lspVersion);
        }
      })
      .catch((err: unknown) => console.error('LSP init error:', err));

    // Listen for diagnostics events from the backend
    createDiagnosticsListener((event) => {
      if (!view) return;
      const diagnostics = event.diagnostics
        .map((d) => {
          try {
            return lspDiagnosticToCodeMirror(d, view!.state.doc);
          } catch {
            return null;
          }
        })
        .filter((d): d is Diagnostic => d !== null);

      // Apply diagnostics to the editor via setDiagnostics
      view!.dispatch(setDiagnostics(view!.state, diagnostics));
    }).then((unlisten) => {
      unlistenDiagnostics = unlisten;
    });
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
    clearTimeout(lspDebounceTimer);
    unlistenDiagnostics?.();
    view?.destroy();
  });

  return <div ref={containerRef} class={styles.container} data-testid="editor-container" />;
}
