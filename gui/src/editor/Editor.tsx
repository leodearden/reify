import { onMount, onCleanup, createEffect } from 'solid-js';
import { EditorState, Transaction, type Extension } from '@codemirror/state';
import { EditorView, keymap, lineNumbers } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { bracketMatching, codeFolding, foldGutter, foldKeymap } from '@codemirror/language';
import { autocompletion, closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
import { search, searchKeymap } from '@codemirror/search';
import { linter, setDiagnostics, type Diagnostic } from '@codemirror/lint';
import { reifyEditorTheme, reifyHighlighting } from './editorTheme';
import { reifyLanguage } from './reifyLanguage';
import { updateSource, saveFile, openFile as bridgeOpenFile } from '../bridge';
import { createLspClient } from './lspClient';
import { reifyCompletionSource } from './completions';
import { createDiagnosticsListener, lspDiagnosticToCodeMirror, diagnosticInfoToCmDiagnostic, type CmDiagnostic } from './diagnostics';
import { reifyHoverTooltip } from './hover';
import { reifyGotoDefinition, gotoDefinitionCommand } from './gotoDefinition';
import { createNavHistory } from '../hooks/useNavHistory';
import type { NavEntry } from '../hooks/useNavHistory';
import type { createEditorStore } from '../stores/editorStore';
import type { FileData, SourceLocation, DiagnosticInfo } from '../types';
import { errorMessage } from '../utils/errorClassifier';
import { isSameFile, normalizePath } from '../utils/pathUtils';
import styles from './Editor.module.css';

// Intentionally shared by both the backend source-sync debounce (updateSource)
// and the LSP didChange debounce — both react to the same user-editing signal
// and are designed to fire on the same tick. If these debounces ever need to
// diverge, introduce a second named constant (e.g. LSP_DID_CHANGE_DEBOUNCE_MS).
export const EDITOR_DEBOUNCE_MS = 300;

export interface EditorProps {
  store: ReturnType<typeof createEditorStore>;
  /**
   * Scroll the editor to the given location. No-op if location.file_path does not
   * match the currently active file (compared with URI normalization so that
   * bare paths and file:// URIs are treated as equivalent).
   */
  scrollToLocation?: () => SourceLocation | null;
  onError?: (message: string) => void;
  onOpen?: () => void;
  /**
   * Called when Mod-s finds the active file is externally changed.
   * App owns the conflict-prompt implementation; Editor just delegates.
   * Both App.handleSave and Editor.Mod-s route here so the UX is identical
   * across call sites.
   */
  onSaveConflict?: (file: FileData) => void;
  /**
   * Compile-time diagnostics from the engine (via engineStore.compileDiagnostics).
   * Filtered to the active file and merged with LSP diagnostics in the CodeMirror
   * lint layer so neither channel clobbers the other.
   */
  compileDiagnostics?: DiagnosticInfo[];
}

export function Editor(props: EditorProps) {
  let containerRef!: HTMLDivElement;
  let view: EditorView | undefined;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  let lspDebounceTimer: ReturnType<typeof setTimeout> | undefined;
  let previousActiveFile: string | null = null;
  let lspVersion = 1;
  const fileStates = new Map<string, EditorState>();
  let extensions: Extension[];
  let unlistenDiagnostics: (() => void) | undefined;
  let diagnosticsListenerCancelled = false;
  let fileOpsPromise: Promise<void> = Promise.resolve();
  let destroyed = false;

  // Current URI — updated on file switch, read by LSP extension getters
  let currentUri = 'file:///untitled.ri';

  // Merge sinks for the two diagnostic channels.
  // setDiagnostics replaces the entire lint set on each dispatch, so both
  // producers (LSP listener + compile-diagnostics effect) update their own
  // slot and then call applyMergedDiagnostics() which dispatches their union.
  let lspCmDiagnostics: CmDiagnostic[] = [];
  let compileCmDiagnostics: CmDiagnostic[] = [];

  function applyMergedDiagnostics() {
    if (!view) return;
    const docLength = view.state.doc.length;
    // Guard against stale offsets from BOTH channels: neither the compile-diagnostics
    // effect nor the LSP listener re-runs on every keystroke.  Typing mutates the view
    // doc directly, so if the doc shrinks before fresh diagnostics arrive, stored
    // from/to values may exceed the new doc length and cause setDiagnostics'
    // RangeSet build to throw.  The LSP/compile debounces share EDITOR_DEBOUNCE_MS,
    // so the compile effect can re-dispatch a now-stale lspCmDiagnostics set whose
    // offsets the LSP listener computed against a longer doc.  Filter the merged
    // union against the live doc so neither channel can dispatch stale ranges.
    const validDiags = [...lspCmDiagnostics, ...compileCmDiagnostics].filter(
      (d) => d.from >= 0 && d.from <= d.to && d.to <= docLength,
    );
    view.dispatch(setDiagnostics(view.state, validDiags));
  }

  // Bounded back/forward navigation-history stack (max 50 entries).
  // Pure closure, no SolidJS dependency.
  const navHistory = createNavHistory();

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

    // Named cross-file navigate callback shared by reifyGotoDefinition (Ctrl+Click)
    // and gotoDefinitionCommand (F12). Extracted so both handlers use identical
    // open-and-place logic and cannot diverge.
    const onCrossFileNavigate = (targetUri: string, line: number, character: number): void => {
      const path = normalizePath(targetUri);
      bridgeOpenFile(path)
        .then((fileData) => {
          if (destroyed) return;
          props.store.openFile(fileData);
          // Defer cursor navigation until after SolidJS reactive file-switch
          // effect has run and the EditorView has the new document.
          setTimeout(() => {
            if (view && !destroyed) {
              const lineNum = line + 1;
              if (lineNum >= 1 && lineNum <= view.state.doc.lines) {
                const targetLine = view.state.doc.line(lineNum);
                const targetPos = Math.min(targetLine.from + character, targetLine.to);
                view.dispatch({
                  selection: { anchor: targetPos },
                  scrollIntoView: true,
                });
              }
            }
          }, 0);
        })
        .catch((err: unknown) => console.error('Cross-file goto-definition error:', err));
    };

    // Nav-history record hook: push origin then destination on a successful
    // same-file goto-definition.  Called by both reifyGotoDefinition (Ctrl+Click)
    // and gotoDefinitionCommand (F12).  Consecutive-dedupe in navHistory makes
    // re-pushes of the same entry idempotent.
    const onRecordJump = (origin: NavEntry, dest: NavEntry): void => {
      navHistory.push(origin);
      navHistory.push(dest);
    };

    // Apply a NavEntry by placing the cursor at entry.offset (same-file only).
    //
    // Navigation history is intentionally scoped to same-file positions in this
    // task (phase ζ): all push sites use `currentUri` — onRecordJump records
    // same-file goto-def jumps, and the scrollToLocation effect records same-file
    // cross-pane reveals.  Cross-file jumps (Ctrl+Click / F12 to another file)
    // are NOT yet tracked; when cross-file push support lands it will wire origin
    // and destination through onCrossFileNavigate and add the cross-file branch
    // here.  Until then, every entry.uri is guaranteed to equal currentUri, so
    // the else branch would be dead code and is intentionally omitted.
    //
    // Pushes never happen here — only goto-def and scrollToLocation push.
    const applyNavEntry = (entry: NavEntry): void => {
      if (!view) return;
      const docLen = view.state.doc.length;
      const offset = Math.min(Math.max(0, entry.offset), docLen);
      view.dispatch({ selection: { anchor: offset }, scrollIntoView: true });
    };

    // Extract extensions into a shared variable for reuse when creating
    // fresh EditorState instances for newly opened files
    extensions = [
      reifyLanguage(),
      lineNumbers(),
      codeFolding(),
      foldGutter(),
      bracketMatching(),
      closeBrackets(),
      reifyEditorTheme,
      reifyHighlighting,
      history(),
      // LSP-powered completions — dynamic URI getter resolves on each request
      autocompletion({ override: [reifyCompletionSource(() => currentUri)] }),
      // LSP-powered hover tooltips — dynamic URI getter
      reifyHoverTooltip(() => currentUri),
      // LSP-powered go-to-definition (Ctrl+Click) — dynamic URI getter
      reifyGotoDefinition(() => currentUri, onCrossFileNavigate, onRecordJump),
      // Find/replace (Ctrl+F, Ctrl+H)
      search(),
      // Diagnostic linter (diagnostics are pushed from LSP via Tauri events)
      linter(() => [] as Diagnostic[]),
      // Navigation keymap — registered BEFORE the defaultKeymap keymap.of so
      // F12 and Alt-Arrow bindings take higher precedence over word-group motion.
      keymap.of([
        {
          key: 'F12',
          run: gotoDefinitionCommand(() => currentUri, onCrossFileNavigate, onRecordJump),
          preventDefault: true,
        },
        {
          key: 'Alt-ArrowLeft',
          run: () => {
            const entry = navHistory.back();
            if (entry) applyNavEntry(entry);
            return true; // always consume — prevents browser back-navigation
          },
          preventDefault: true,
        },
        {
          key: 'Alt-ArrowRight',
          run: () => {
            const entry = navHistory.forward();
            if (entry) applyNavEntry(entry);
            return true; // always consume — prevents browser forward-navigation
          },
          preventDefault: true,
        },
      ]),
      keymap.of([
        {
          key: 'Mod-o',
          run: () => {
            props.onOpen?.();
            return true;
          },
          preventDefault: true,
        },
        {
          key: 'Mod-s',
          run: () => {
            const path = props.store.state.activeFile;
            if (!path) return true;
            const result = props.store.canSave(path);
            if (!result.ok) {
              switch (result.reason) {
                case 'not-found':
                  // Invariant breach — activeFile/path should always be in openFiles.
                  // Do not surface a toast since this is not an actionable user condition.
                  // Mirrors App.tsx#handleSave so both Ctrl+S call sites have identical
                  // user-visible policy.
                  console.error('Save aborted: file not in store', path);
                  return true;
                case 'externally-changed': {
                  // canSave checks 'not-found' first, so when we reach this
                  // branch the file is guaranteed in openFiles. The `if (file)`
                  // guard is belt-and-braces — no non-null assertion / no `!`.
                  const file = props.store.state.openFiles.find((f) => f.path === path);
                  if (file) props.onSaveConflict?.(file);
                  return true;
                }
                default: {
                  // Exhaustiveness guard: TypeScript flags this `: never` assignment
                  // as a compile error if a new SaveBlockedReason member is added
                  // without updating this switch.  Runtime path: emits a console.error
                  // breadcrumb, surfaces a props.onError toast ("Save failed: internal
                  // error"), then returns true so CM6 swallows the keystroke and the
                  // browser's native Mod-s dialog does not leak through.
                  // Intentionally diverges from the 'not-found' arm, which suppresses
                  // the toast: reaching this arm implies a contract violation — a new
                  // SaveBlockedReason added without updating this switch — that a
                  // maintainer should learn about, whereas 'not-found' is a
                  // known-transient invariant breach with no actionable user message.
                  const _exhaustive: never = result.reason;
                  console.error('unhandled save-blocked reason:', _exhaustive);
                  props.onError?.('Save failed: internal error');
                  return true;
                }
              }
            }
            saveFile(result.file.path, result.file.content)
              .then(() => props.store.markClean(result.file.path))
              .catch((err: unknown) =>
                props.onError?.(`Failed to save file: ${errorMessage(err)}`),
              );
            return true;
          },
          preventDefault: true,
        },
        ...closeBracketsKeymap,
        ...searchKeymap,
        ...defaultKeymap,
        ...historyKeymap,
      ]),
      keymap.of(foldKeymap),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          // Bail out for sync-external transactions — these originate from the
          // in-file content-sync effect (auto-reload, handleReload) and must be
          // invisible to the dirty-tracking + backend-sync pipeline. Without this
          // bail, every auto-reload would: (1) immediately re-mark the file dirty
          // after markClean, and (2) echo the just-pushed content back to the
          // backend as a phantom user edit via updateSource.
          const isSyncOrigin = update.transactions.some(
            (t) => t.annotation(Transaction.userEvent)?.startsWith('sync.external'),
          );
          if (isSyncOrigin) return;

          const path = props.store.state.activeFile;
          if (path) {
            props.store.markDirty(path);
            clearTimeout(debounceTimer);
            debounceTimer = setTimeout(() => {
              updateSource(path, update.state.doc.toString()).catch((err: unknown) =>
                console.error('Failed to update source:', err),
              );
            }, EDITOR_DEBOUNCE_MS);

            // Send didChange to LSP (debounced)
            clearTimeout(lspDebounceTimer);
            lspDebounceTimer = setTimeout(() => {
              lspVersion++;
              lspClient
                .didChange(pathToUri(path), update.state.doc.toString(), lspVersion)
                .catch((err: unknown) => console.error('LSP didChange error:', err));
            }, EDITOR_DEBOUNCE_MS);
          }
        }
        if (update.selectionSet) {
          const pos = update.state.selection.main.head;
          const line = update.state.doc.lineAt(pos);
          // Emit 1-based column to match the backend convention required by
          // getEntityAtSourceLocation (engine.rs:2227, documented 1-based at
          // engine.rs:2208) and getContainingDefinition (engine.rs:2153,
          // documented 1-based at engine.rs:2134). CodeMirror's `pos - line.from`
          // is 0-based; adding 1 converts to 1-based codepoint offset.
          props.store.setCursorPosition(line.number, pos - line.from + 1);
        }
      }),
    ];

    const state = EditorState.create({ doc, extensions });

    view = new EditorView({ state, parent: containerRef });

    // Expose editor view for the debug bridge (REIFY_DEBUG=1)
    if (window.__REIFY_DEBUG__) {
      window.__REIFY_DEBUG__.editorView = view;
    }

    // Initialize LSP, send 'initialized' notification, then open the document
    lspClient
      .initialize()
      .then(() => lspClient.initialized())
      .then(() => {
        if (activeFile) {
          return lspClient.didOpen(currentUri, doc, lspVersion);
        }
      })
      .catch((_err: unknown) =>
        props.onError?.('LSP initialization failed — completions and diagnostics may be unavailable'),
      );

    // Listen for diagnostics events from the backend.
    // Use a cancelled flag to handle the race where onCleanup fires
    // before the listen promise resolves — prevents leaking the
    // Tauri event listener.
    createDiagnosticsListener((event) => {
      if (!view) return;
      // Only apply diagnostics for the currently active file
      if (event.uri !== currentUri) return;
      lspCmDiagnostics = event.diagnostics
        .map((d) => {
          try {
            return lspDiagnosticToCodeMirror(d, view!.state.doc);
          } catch {
            return null;
          }
        })
        .filter((d): d is CmDiagnostic => d !== null);

      // Merge with compile diagnostics and dispatch
      applyMergedDiagnostics();
    }).then((unlisten) => {
      if (diagnosticsListenerCancelled) {
        unlisten?.(); // Component already unmounted — tear down immediately
      } else {
        unlistenDiagnostics = unlisten;
      }
    });
  });

  // Watch for active file changes and swap document content
  createEffect(() => {
    const activeFile = props.store.state.activeFile;
    if (!view || activeFile === previousActiveFile) return;

    // Cancel any pending debounced operations from the previous file
    clearTimeout(debounceTimer);
    clearTimeout(lspDebounceTimer);

    // Discard previous-file CmDiagnostics from both channels.  Their from/to
    // offsets were computed against the old document; re-using them after the view
    // switches to the new (possibly shorter) document would inject phantom squiggles
    // (FLASH) or crash the CodeMirror RangeSet build (CRASH).  Both slots are reset
    // here so the clearing is self-contained — correctness does not depend on the
    // compile-diagnostics effect running after this one.  The URI-guarded LSP listener
    // repopulates lspCmDiagnostics when the server re-publishes for the new file; the
    // compile-diagnostics effect recomputes compileCmDiagnostics on the activeFile change.
    lspCmDiagnostics = [];
    compileCmDiagnostics = [];

    const oldUri = currentUri;
    previousActiveFile = activeFile;

    const file = props.store.state.openFiles.find((f) => f.path === activeFile);
    const newContent = file?.content ?? '';
    const newUri = activeFile ? pathToUri(activeFile) : 'file:///untitled.ri';

    // Update the mutable URI so extension getters resolve to the new file
    currentUri = newUri;

    // Save current file's EditorState (keyed by URI) before switching
    fileStates.set(oldUri, view.state);

    // Restore or create EditorState for the new file
    const savedState = fileStates.get(newUri);
    if (savedState) {
      view.setState(savedState);
    } else {
      view.setState(EditorState.create({ doc: newContent, extensions }));
    }

    // Dispatch the now-empty merged set so no stale squiggle persists after the
    // document replacement above.  Self-contained — no cross-effect ordering needed.
    applyMergedDiagnostics();

    // Close old document and open new one in the LSP server.
    // Chain off fileOpsPromise to serialize rapid file switches.
    lspVersion++;
    const version = lspVersion;
    fileOpsPromise = fileOpsPromise
      .then(() => lspClient.didClose(oldUri))
      .then(() => {
        if (destroyed) return;
        return lspClient.didOpen(newUri, newContent, version);
      })
      .catch((err: unknown) => console.error('LSP file switch error:', err));
  });

  // Map compile diagnostics from the engine store into the CodeMirror lint layer,
  // merged with the existing LSP channel so neither clobbers the other.
  createEffect(() => {
    const diags = props.compileDiagnostics;
    const activeFile = props.store.state.activeFile;
    if (!view) return;
    compileCmDiagnostics = (diags ?? [])
      .filter((d) => activeFile && isSameFile(d.file_path, activeFile))
      .map((d) => {
        try {
          return diagnosticInfoToCmDiagnostic(d, view!.state.doc);
        } catch {
          return null;
        }
      })
      .filter((d): d is CmDiagnostic => d !== null);
    applyMergedDiagnostics();
  });

  // Sync store content → CodeMirror view for the active file.
  //
  // This effect fires when the store's file.content changes externally (auto-reload,
  // handleReload) for the active file. It intentionally bails when the active file
  // changes (file switch) so the file-switch effect above can restore the cached
  // EditorState — which may contain unsaved user edits that the store doesn't hold.
  //
  // Anti-loop invariant: user typing dispatches changes directly to the view
  // via the EditorView.updateListener but does NOT call updateFileContent —
  // only the debounced updateSource (backend call) is made. So file.content
  // (the reactive signal) never changes during typing, and this effect never
  // re-runs during a typing session.
  //
  // Subscription discipline: we always read file.content before any early return
  // so that the reactive subscription is established for the current active file;
  // this ensures the effect re-fires when updateFileContent is called even if a
  // prior run bailed (e.g., because the view wasn't mounted yet).
  let syncPreviousActive: string | null = null;
  createEffect(() => {
    const activeFile = props.store.state.activeFile;

    // Always read file.content before any early return to maintain the reactive
    // subscription for the current active file.
    const file = activeFile
      ? props.store.state.openFiles.find((f) => f.path === activeFile)
      : undefined;
    const storeContent = file?.content;

    // Not mounted yet or no active file — update tracking to avoid a spurious
    // dispatch on the first run after mount.
    if (!view || !activeFile || !file || storeContent === undefined) {
      syncPreviousActive = activeFile;
      return;
    }

    // Active file just changed — the file-switch effect above handles EditorState
    // rebuild (restoring cached state which may include unsaved user edits).
    // Update tracking and bail without dispatching.
    if (activeFile !== syncPreviousActive) {
      syncPreviousActive = activeFile;
      return;
    }

    // Same active file, store content changed externally (e.g. auto-reload).
    // Dispatch only when there is an actual diff to prevent no-op transactions.
    //
    // The dispatch is doubly-protected:
    // 1. Transaction.userEvent.of('sync.external') — the updateListener checks this
    //    annotation and bails before calling markDirty + updateSource (anti-loop).
    //    User typing produces normal user-event transactions; the bail does NOT fire
    //    during typing, preserving the dirty-tracking pipeline for real edits.
    // 2. Transaction.addToHistory.of(false) — excludes the transaction from
    //    CodeMirror's undo stack so Ctrl+Z cannot revive the pre-reload stale buffer.
    if (view.state.doc.toString() !== storeContent) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: storeContent },
        annotations: [
          Transaction.userEvent.of('sync.external'),
          Transaction.addToHistory.of(false),
        ],
      });
    }
  });

  // Watch scrollToLocation signal and scroll editor to target location
  createEffect(() => {
    const location = props.scrollToLocation?.();
    if (!view || !location) return;
    if (!isSameFile(location.file_path, props.store.state.activeFile ?? '')) return;

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

    // Record nav history: push origin (current cursor) then destination (anchor).
    // Consecutive-dedupe in navHistory makes this idempotent under SolidJS
    // effect re-runs (e.g., when the activeFile signal changes with an unchanged
    // scrollToLocation value) — no spurious duplicate entries are created.
    const originOffset = view.state.selection.main.head;
    navHistory.push({ uri: currentUri, offset: originOffset });
    navHistory.push({ uri: currentUri, offset: anchor });

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
    // Release cached per-file EditorState instances
    fileStates.clear();
    // Prevent any in-flight file switch chain from calling didOpen after teardown
    destroyed = true;
    // Chain the final didClose off fileOpsPromise so it waits for any
    // in-flight file switch operations to complete before closing
    const uriToClose = currentUri;
    fileOpsPromise = fileOpsPromise
      .then(() => lspClient.didClose(uriToClose))
      .catch(() => {});
    if (window.__REIFY_DEBUG__) {
      delete window.__REIFY_DEBUG__.editorView;
    }
    view?.destroy();
  });

  return <div ref={containerRef} class={styles.container} data-testid="editor-container" />;
}
