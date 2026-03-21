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
import { createDiagnosticsListener, lspDiagnosticToCodeMirror, type CmDiagnostic } from './diagnostics';
import { reifyHoverTooltip } from './hover';
import { reifyGotoDefinition } from './gotoDefinition';
import type { createEditorStore } from '../stores/editorStore';
import type { SourceLocation } from '../types';
import styles from './Editor.module.css';

export interface EditorProps {
  store: ReturnType<typeof createEditorStore>;
  scrollToLocation?: () => SourceLocation | null;
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
  let diagnosticsListenerCancelled = false;

  // Current URI — updated on file switch, read by LSP extension getters
  let currentUri = 'file:///untitled.ri';

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
    currentUri = activeFile ? pathToUri(activeFile) : 'file:///untitled.ri';

    const state = EditorState.create({
      doc,
      extensions: [
        reifyLanguage(),
        bracketMatching(),
        syntaxHighlighting(defaultHighlightStyle),
        history(),
        // LSP-powered completions — dynamic URI getter resolves on each request
        autocompletion({ override: [reifyCompletionSource(() => currentUri)] }),
        // LSP-powered hover tooltips — dynamic URI getter
        reifyHoverTooltip(() => currentUri),
        // LSP-powered go-to-definition (Ctrl+Click) — dynamic URI getter
        reifyGotoDefinition(() => currentUri),
        // Diagnostic linter (diagnostics are pushed from LSP via Tauri events)
        linter(() => [] as Diagnostic[]),
        keymap.of([
          {
            key: 'Mod-s',
            run: () => {
              const path = props.store.state.activeFile;
              if (path) {
                const file = props.store.state.openFiles.find((f) => f.path === path);
                const content = file?.content ?? '';
                saveFile(path, content)
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

    // Initialize LSP, send 'initialized' notification, then open the document
    lspClient
      .initialize()
      .then(() => lspClient.initialized())
      .then(() => {
        if (activeFile) {
          return lspClient.didOpen(currentUri, doc, lspVersion);
        }
      })
      .catch((err: unknown) => console.error('LSP init error:', err));

    // Listen for diagnostics events from the backend.
    // Use a cancelled flag to handle the race where onCleanup fires
    // before the listen promise resolves — prevents leaking the
    // Tauri event listener.
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
        .filter((d): d is CmDiagnostic => d !== null);

      // Apply diagnostics to the editor via setDiagnostics
      view!.dispatch(setDiagnostics(view!.state, diagnostics));
    }).then((unlisten) => {
      if (diagnosticsListenerCancelled) {
        unlisten(); // Component already unmounted — tear down immediately
      } else {
        unlistenDiagnostics = unlisten;
      }
    });
  });

  // Watch for active file changes and swap document content
  createEffect(() => {
    const activeFile = props.store.state.activeFile;
    if (!view || activeFile === previousActiveFile) return;

    const oldUri = currentUri;
    previousActiveFile = activeFile;

    const file = props.store.state.openFiles.find((f) => f.path === activeFile);
    const newContent = file?.content ?? '';
    const newUri = activeFile ? pathToUri(activeFile) : 'file:///untitled.ri';

    // Update the mutable URI so extension getters resolve to the new file
    currentUri = newUri;

    isSwitching = true;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: newContent },
      selection: { anchor: 0 },
    });
    isSwitching = false;

    // Close old document and open new one in the LSP server
    lspVersion++;
    lspClient
      .didClose(oldUri)
      .then(() => lspClient.didOpen(newUri, newContent, lspVersion))
      .catch((err: unknown) => console.error('LSP file switch error:', err));
  });

  // Watch scrollToLocation signal and scroll editor to target location
  createEffect(() => {
    const location = props.scrollToLocation?.();
    if (!view || !location) return;

    const doc = view.state.doc;
    const lineCount = doc.lines;

    // Guard against out-of-range positions
    if (location.line < 1 || location.line > lineCount) return;

    const line = doc.line(location.line);
    const anchor = Math.min(line.from + (location.column - 1), line.to);

    let head = anchor;
    if (location.end_line >= 1 && location.end_line <= lineCount) {
      const endLine = doc.line(location.end_line);
      head = Math.min(endLine.from + (location.end_column - 1), endLine.to);
    }

    view.dispatch({
      selection: { anchor, head },
      scrollIntoView: true,
    });
  });

  onCleanup(() => {
    clearTimeout(debounceTimer);
    clearTimeout(lspDebounceTimer);
    // Mark diagnostics listener as cancelled so that if the listen
    // promise hasn't resolved yet, it will call unlisten() immediately
    // when it does resolve (preventing a leaked Tauri event listener).
    diagnosticsListenerCancelled = true;
    unlistenDiagnostics?.();
    // Close the current document in the LSP server
    lspClient.didClose(currentUri).catch(() => {});
    view?.destroy();
  });

  return <div ref={containerRef} class={styles.container} data-testid="editor-container" />;
}
