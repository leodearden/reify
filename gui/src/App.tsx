import { type Component, onMount, onCleanup, createSignal, createEffect, createMemo, Show, For, untrack, batch } from 'solid-js';
import { DualViewport } from './viewport';
import { Editor } from './editor/Editor';
import { FileTabs } from './editor/FileTabs';
import {
  PropertyEditor,
  ConstraintPanel,
  Toolbar,
  StatusBar,
  FileBrowser,
  ExportDialog,
  Toast,
  ReloadPrompt,
  ChatPanel,
  MenuBar,
  DesignTree,
  ViewManageModal,
  MechanismPanel,
  DiagnosticsPanel,
  AutoResolvePanel,
  SolverProgressOverlay,
} from './panels';
import type { DiagnosticEntry } from './panels';
import { WarmPoolDebugPanel } from './debug/WarmPoolDebugPanel';
import { Splitter } from './components/Splitter';
import { KeyboardHelp } from './components/KeyboardHelp';
import { useKeyboardShortcuts } from './hooks/useKeyboardShortcuts';
import { createEngineStore } from './stores/engineStore';
import { createEditorStore } from './stores/editorStore';
import { createSelectionStore } from './stores/selectionStore';
import { createClaudeStore } from './stores/claudeStore';
import { createViewStateStore } from './stores/viewStateStore';
import { createViewportStore, type CameraState } from './stores/viewportStore';
import { createDefPreviewStore } from './stores/defPreviewStore';
import { createMechanismStore } from './stores/mechanismStore';
import { createDefPreviewActivation } from './hooks/useDefPreviewActivation';
import { createEditorSelectionSync } from './hooks/useEditorSelectionSync';
import {
  getInitialState,
  getEntityTree as bridgeGetEntityTree,
  setParameter as bridgeSetParameter,
  exportGeometry as bridgeExportGeometry,
  pickSavePath,
  pickOpenPath,
  updateSource as bridgeUpdateSource,
  openFile as bridgeOpenFile,
  openFileEngine as bridgeOpenFileEngine,
  saveFile as bridgeSaveFile,
  onFileChanged,
  onFileRemoved,
  onSerializationError,
  onFocusEntity,
  onNavigateToSource,
  getSourceLocation as bridgeGetSourceLocation,
  focusEntity as bridgeFocusEntity,
  claudeSendMessage,
  claudeAbort,
  claudePermissionDecision,
  subscribeToClaudeEvents,
  subscribeToSidecarCrashed,
  isDebugEnabled,
  getKernelStatus,
  onKernelStatus,
  getContainingDefinition as bridgeGetContainingDefinition,
  getEntityAtSourceLocation as bridgeGetEntityAtSourceLocation,
  getDefPreview as bridgeGetDefPreview,
  getMechanismDescriptors as bridgeGetMechanismDescriptors,
} from './bridge';
import {
  navigateToSource,
  navigateToEntity,
  navigateFromConstraint,
} from './navigation';
import type { ExportFormat, FileData, SourceLocation, ConstraintData, ToastMessage, ToastAction, EntityTreeNode } from './types';
import { applyTheme } from './theme';
import { errorMessage } from './utils/errorClassifier';
import { isSameFile } from './utils/pathUtils';
import {
  messageForSaveBlocked,
  EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG,
  SAVE_CONFLICT_RELOAD_LABEL,
  SAVE_CONFLICT_OVERWRITE_LABEL,
} from './editor/messages';
import { loadPanelLayout, savePanelLayout, clampPanelHeightsToFit } from './hooks/useLayoutPersistence';
import { createSerializationErrorCoalescer } from './hooks/useSerializationErrorCoalescer';
import { loadSidecar, saveSidecar } from './stores/sidecarPersistence';
import { loadViewPersistence, createDebouncedSaver, type DebouncedSaver } from './stores/viewPersistence';
import { findFuzzyCandidate } from './stores/fuzzyPathMatcher';
import type { PersistentViewState } from './types';
import styles from './App.module.css';

export const NEW_FILE_TEMPLATE = '// New design\n';
const MIN_PANEL_WIDTH = 150;
const MIN_PANEL_HEIGHT = 80;
const DEFAULT_EDITOR_WIDTH = 300;
const DEFAULT_SIDE_WIDTH = 300;
const DEFAULT_DESIGN_TREE_HEIGHT = 160;
const DEFAULT_PROPERTY_HEIGHT = 200;
const DEFAULT_CONSTRAINT_HEIGHT = 140;
const CHAT_MIN_HEIGHT = 160;
const SPLITTER_THICKNESS = 4;

let toastIdCounter = 0;

const App: Component = () => {
  const editorStore = createEditorStore();
  const selectionStore = createSelectionStore();
  const engineStore = createEngineStore({
    onEntityRemoved: (id) => selectionStore.clearIfRemoved(id),
    onEngineReinitialized: () => {
      refreshEntityTree();
      mechanismStore.refresh().catch((err) =>
        console.error('[mechanism] refresh failed:', err),
      );
    },
  });
  const claudeStore = createClaudeStore({
    onSend: (_id, text, context) => {
      claudeSendMessage(text, context).catch((err) => {
        const msg = errorMessage(err);
        claudeStore.addSystemMessage('ipc_error', `Failed to send message: ${msg}`);
      });
    },
    onAbort: () => {
      claudeAbort().catch((err) => {
        console.error('[claude] abort failed:', err);
        showToast(`Abort failed: ${errorMessage(err)}`, 'error');
      });
    },
    onPermissionDecision: ({ requestId, behavior, message, updatedInput, remember }) => {
      claudePermissionDecision({ requestId, behavior, message, updatedInput, remember }).catch((err) => {
        showToast(`Permission decision failed: ${errorMessage(err)}`, 'error');
      });
    },
  });

  const viewStateStore = createViewStateStore();
  const viewportStore = createViewportStore();
  const defPreviewStore = createDefPreviewStore();
  const mechanismStore = createMechanismStore({ getMechanismDescriptors: bridgeGetMechanismDescriptors });

  // Track the currently-open file path so the debounced save effect can key off it.
  const [currentFilePath, setCurrentFilePath] = createSignal<string | null>(null);

  // Fuzzy-rebind toast bookkeeping (see the rebind effect block below).
  //
  // `rebindShownPairs` tracks stale→candidate pairs currently represented by
  // a visible toast. Without this, every re-evaluation that still shows the
  // same stale path enqueues another identical toast, which reviewers flagged
  // as a growing stack when users do not promptly respond.
  //
  // `rebindToastPairs` maps toast-id → pairKey so `handleDismissToast` can
  // clear the pair from `rebindShownPairs` regardless of how the toast was
  // dismissed (button click, close-X, or auto-dismiss timeout).
  //
  // Both live at App scope so `handleDismissToast` (declared further below)
  // can see them.
  const rebindShownPairs = new Set<string>();
  const rebindToastPairs = new Map<string, string>();

  /**
   * Load view state for a given file path.
   * Priority: sidecar (.ri.views.json) > localStorage > null (defaults).
   * Stops at the first non-null valid layer (PRD §8.1 design decision).
   */
  async function loadPersistedViews(path: string): Promise<PersistentViewState | null> {
    const sidecar = await loadSidecar(path);
    if (sidecar !== null) return sidecar;
    return loadViewPersistence(path);
  }

  // Debounced persistence of view state to localStorage.
  //
  // A single `DebouncedSaver` is retained across view-state mutations for the
  // same path so that rapid changes coalesce into one write 500ms after the
  // last mutation (PRD §8.1 design decision).
  //
  // Lifecycle:
  // - File open / file switch → flush the outgoing saver (if any), then
  //   create a fresh saver for the new path.  Flushing on switch is required
  //   so the last mutation made to the outgoing file is not lost when the
  //   user switches files within the debounce window.
  // - Component unmount → flush any pending write in onCleanup.
  //
  // The effect re-runs on every change to `viewStateStore.state` or
  // `viewportStore.state`; those re-runs only reschedule on the existing
  // saver (they do NOT recreate it), which preserves the debounce window.
  {
    let activeSaver: DebouncedSaver | null = null;
    let activePath: string | null = null;

    createEffect(() => {
      const path = currentFilePath();

      // Path transition: flush pending writes for the old path, then swap
      // the saver.  Comparing `path !== activePath` ensures unrelated
      // view-state changes reuse the current saver.
      if (path !== activePath) {
        activeSaver?.flush();
        activeSaver = path !== null ? createDebouncedSaver(500) : null;
        activePath = path;
      }

      if (!path || !activeSaver) return;

      // Reactive subscriptions come from the property reads below:
      // `Object.entries(viewportStore.state.viewports)` subscribes to
      // viewport-store mutations, and `viewStateStore.serializePersistedState()`
      // walks active-view / user-views / explicit overrides inside the store
      // which subscribes to view-state mutations.  (Reading just the root
      // `.state` property does NOT track nested mutations in Solid stores —
      // only property access does.)
      const viewportCameras: Record<string, CameraState> = {};
      for (const [id, vp] of Object.entries(viewportStore.state.viewports)) {
        if (vp.camera) viewportCameras[id] = vp.camera;
      }

      const composed: PersistentViewState = {
        ...viewStateStore.serializePersistedState(),
        viewportCameras,
        timestamp: new Date().toISOString(),
      };

      activeSaver.schedule(path, composed);
    });

    onCleanup(() => {
      // Component unmount: persist any still-pending mutation rather than
      // dropping it (the previous cancel()-on-cleanup silently lost writes
      // when unmount/file-switch raced the 500ms timer).
      activeSaver?.flush();
      activeSaver = null;
      activePath = null;
    });
  }

  // Activation hook: watches editor cursor → debounces 200ms → loads def preview
  const defPreviewActivation = createDefPreviewActivation({
    editorStore,
    viewportStore,
    defPreviewStore,
    getContainingDefinition: bridgeGetContainingDefinition,
    getDefPreview: bridgeGetDefPreview,
    debounceMs: 200,
  });

  // Editor→entity sync: watches editor cursor → debounces 200ms → resolves entity
  // at cursor position → updates selectionStore + flies to entity in viewport.
  // Equality-check guard prevents viewport-click → editor-scroll → cursor-move bounce.
  createEditorSelectionSync({
    editorStore,
    selectionStore,
    getEntityAtSourceLocation: bridgeGetEntityAtSourceLocation,
    selectEntity: (ep) => selectionStore.selectEntity(ep),
    flyToEntity: (ep) => flyToEntityFn?.(ep),
    debounceMs: 200,
  });

  // One-way sync: keep viewportStore["design-main"].viewId in step with the
  // active view chosen by the user (via ViewSelector / DesignTree / keyboard shortcuts).
  // This satisfies PRD §3.2 — viewportStore is the authoritative per-viewport view
  // assignment, while viewStateStore remains the authoritative view-tree/visibility store.
  createEffect(() => {
    viewportStore.assignView('design-main', viewStateStore.state.activeViewId);
  });

  const [entityTree, setEntityTree] = createSignal<EntityTreeNode[]>([]);

  function refreshEntityTree(): void {
    bridgeGetEntityTree()
      .then((t) => { if (alive) setEntityTree(t); })
      .catch((err) => console.error('[entity-tree] refresh failed:', err));
  }

  // Reactive counter incremented each time viewStateStore.setTree is called.
  // This lets the effectiveVisibility memo re-evaluate AFTER nodeByPath is rebuilt,
  // avoiding a race where the memo re-runs before the createEffect below has executed.
  const [treeGeneration, setTreeGeneration] = createSignal(0);

  // Keep viewStateStore in sync with the latest entity tree.
  // regenerateAutoViews handles both the nodeByPath rebuild (equivalent to
  // setTree) AND auto-view generation in one reactive notification.
  // Increment treeGeneration AFTER regenerateAutoViews so that effectiveVisibility
  // always evaluates getAllEffective() with an up-to-date nodeByPath.
  createEffect(() => {
    viewStateStore.regenerateAutoViews(entityTree());
    setTreeGeneration((v) => v + 1);
  });

  // Fuzzy-rebind notification: after each tree update, check for stale paths
  // that have a single unambiguous suffix-match candidate and surface a
  // non-blocking toast with [Yes][No][Ignore] actions.
  // Per PRD §8.5: never auto-applies — user must confirm explicitly.
  {
    // Session-scoped set of ignored stale→candidate pairs.
    // Keyed by "${stalePath}→${newPath}" to suppress re-prompts after [No]/[Ignore].
    const ignoredPairs = new Set<string>();

    createEffect(() => {
      void treeGeneration(); // re-run after each tree update
      const tree = untrack(() => entityTree());
      const stalePaths = untrack(() => viewStateStore.getStalePaths());

      for (const stalePath of stalePaths) {
        const candidate = findFuzzyCandidate(stalePath, null, tree);
        if (!candidate) continue;

        const pairKey = `${stalePath}→${candidate.path}`;
        if (ignoredPairs.has(pairKey)) continue;
        // Reviewer fix: skip when an outstanding toast already represents
        // this pair.  Without this guard, any subsequent tree update that
        // leaves the stale path stale (e.g. an unrelated edit) enqueues a
        // duplicate toast, producing a growing stack for users who do not
        // respond immediately.
        if (rebindShownPairs.has(pairKey)) continue;

        // Snapshot the stale path's explicit visibility before the closure captures it.
        const staleVisibility = untrack(() => viewStateStore.state.explicit[stalePath]);

        const candidatePath = candidate.path;
        rebindShownPairs.add(pairKey);
        const toastId = showToast(
          `"${stalePath}" may have been renamed to "${candidatePath}". Rebind?`,
          'info',
          [
            {
              label: 'Yes',
              onClick: () => {
                // Transfer the stale path's visibility to the new path.
                if (staleVisibility) {
                  viewStateStore.setVisibility(candidatePath, staleVisibility);
                }
                // Remove the stale explicit entry.
                viewStateStore.resetToInherit(stalePath);
                // rebindShownPairs is cleared in handleDismissToast (via the
                // toast-id → pairKey map) when the button's onDismiss fires.
              },
            },
            {
              label: 'No',
              onClick: () => {
                // Dismiss and suppress this specific pair for the session —
                // the stale entry stays so the user can still rebind manually
                // or undo. This matches [Ignore] scoping: the same stale→candidate
                // pair will not re-fire on the next tree update.
                ignoredPairs.add(pairKey);
              },
            },
            {
              label: 'Ignore',
              onClick: () => {
                // Suppress this pair for the rest of the session.
                ignoredPairs.add(pairKey);
              },
            },
          ],
        );
        // Remember which pair this toast represents so handleDismissToast
        // can clean up rebindShownPairs when the toast goes away for ANY
        // reason (button, close-X, or auto-dismiss timeout).
        rebindToastPairs.set(toastId, pairKey);
      }
    });
  }

  // True when at least one design mesh is loaded. Memoized so DualViewport's
  // designViewportActive prop does not allocate a new array on every reactive pulse.
  const hasMeshes = createMemo(() => Object.keys(engineStore.state.meshes).length > 0);

  // Effective visibility memo: re-evaluates whenever explicit overrides or treeGeneration
  // changes.  treeGeneration is incremented by the effect above after setTree runs, which
  // guarantees that getAllEffective() sees the up-to-date nodeByPath on every call.
  const effectiveVisibility = createMemo(() => {
    void treeGeneration(); // track treeGeneration so the memo re-runs after setTree
    return viewStateStore.getAllEffective();
  });

  // Re-fetch entity tree on transitions from any non-idle phase back to 'idle'.
  // prevPhase starts as undefined so the first effect run (which just reads the
  // initial phase) never triggers a fetch — only genuine non-idle→idle transitions
  // do. This avoids races with initApp's explicit fetch regardless of what phase
  // the engine reports during initialisation.
  {
    let prevPhase: string | undefined;
    createEffect(() => {
      const phase = engineStore.state.evalStatus.phase;
      if (prevPhase !== undefined && phase === 'idle' && prevPhase !== 'idle') {
        refreshEntityTree();
      }
      prevPhase = phase;
    });
  }

  // Re-fetch mechanism descriptors on each non-idle→idle transition.
  // Mirrors the entity-tree refresh effect above.
  {
    let prevPhase: string | undefined;
    createEffect(() => {
      const phase = engineStore.state.evalStatus.phase;
      if (prevPhase !== undefined && phase === 'idle' && prevPhase !== 'idle') {
        mechanismStore.refresh()
          .catch((err) => console.error('[mechanism] refresh failed:', err));
      }
      prevPhase = phase;
    });
  }

  const savedLayout = loadPanelLayout();
  const [editorWidth, setEditorWidth] = createSignal(savedLayout?.editorWidth ?? DEFAULT_EDITOR_WIDTH);
  const [sideWidth, setSideWidth] = createSignal(savedLayout?.sideWidth ?? DEFAULT_SIDE_WIDTH);
  const [designTreeHeight, setDesignTreeHeight] = createSignal(savedLayout?.designTreeHeight ?? DEFAULT_DESIGN_TREE_HEIGHT);
  const [propertyHeight, setPropertyHeight] = createSignal(savedLayout?.propertyHeight ?? DEFAULT_PROPERTY_HEIGHT);
  const [constraintHeight, setConstraintHeight] = createSignal(savedLayout?.constraintHeight ?? DEFAULT_CONSTRAINT_HEIGHT);

  // Debounced persistence of panel layout dimensions
  let saveTimeout: ReturnType<typeof setTimeout> | undefined;
  createEffect(() => {
    const layout = {
      editorWidth: editorWidth(),
      sideWidth: sideWidth(),
      designTreeHeight: designTreeHeight(),
      propertyHeight: propertyHeight(),
      constraintHeight: constraintHeight(),
    };
    clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => savePanelLayout(layout), 300);
  });

  // Init phase: loading → ready | error
  const [initPhase, setInitPhase] = createSignal<'loading' | 'ready' | 'error'>('loading');

  // Chat panel open/closed state
  const [chatOpen, setChatOpen] = createSignal(true);

  // Export dialog state
  const [showExportDialog, setShowExportDialog] = createSignal(false);

  // View manage modal state
  const [viewManageOpen, setViewManageOpen] = createSignal(false);

  // Diagnostics panel state
  const [diagnosticsOpen, setDiagnosticsOpen] = createSignal(false);
  // Both compile and tessellation diagnostics share the DiagnosticInfo schema, so the
  // panel renders them as a single merged list — no schema change or extra state needed.
  // The two pipelines are disjoint by construction: compile errors come from the static
  // analysis pass, tessellation errors from the mesh-generation stage, so no diagnostic
  // can appear in both lists and no deduplication is required.
  // The `source` tag is a frontend-only field (never on the wire from the Rust backend);
  // it is added here at the merge boundary so DiagnosticsPanel can render a per-row
  // chip identifying which pipeline produced each entry.
  const allDiagnostics = createMemo((): DiagnosticEntry[] => [
    ...engineStore.state.compileDiagnostics.map(d => ({ ...d, source: 'compile' as const })),
    ...engineStore.state.tessellationDiagnostics.map(d => ({ ...d, source: 'tessellation' as const })),
  ]);

  // Keyboard help overlay state
  const [showHelp, setShowHelp] = createSignal(false);
  const [exporting, setExporting] = createSignal(false);
  // Gate for REIFY_DEBUG=1 panels (WarmPoolDebugPanel, etc.) — set in initApp()
  const [debugEnabled, setDebugEnabled] = createSignal(false);

  // Toast queue state
  const [toasts, setToasts] = createSignal<ToastMessage[]>([]);

  function showToast(message: string, type: ToastMessage['type'], actions?: ToastAction[]): string {
    const id = String(++toastIdCounter);
    setToasts((prev) => [...prev, { id, type, message, actions }]);
    return id;
  }

  // Coalescer for serialization-error events — debounces and deduplicates bursts
  const serializationErrorCoalescer = createSerializationErrorCoalescer(showToast);

  // Reload prompt state — tracks all files changed since last reload/dismiss
  const [changedFiles, setChangedFiles] = createSignal<Set<string>>(new Set());
  const [confirmReload, setConfirmReload] = createSignal(false);

  // Navigation state
  const [scrollToLocation, setScrollToLocation] = createSignal<SourceLocation | null>(null);
  let flyToEntityFn: ((entityPath: string) => void) | undefined;
  let fitToViewFn: (() => void) | undefined;

  // Both the compile badge and the tessellation badge call this handler — both are
  // toggles. Clicking either badge while the panel is already open will close it rather
  // than forcing it open. This matches the "one shared overlay" design; if a force-open
  // affordance is wanted in the future, split into onShowDiagnostics (setDiagnosticsOpen(true))
  // vs a dedicated keyboard-shortcut toggle.
  function handleToggleDiagnostics() {
    setDiagnosticsOpen((v) => !v);
  }

  function handleNavigateToDiagnostic(d: DiagnosticEntry) {
    setScrollToLocation({ file_path: d.file_path, line: d.line, column: d.column, end_line: d.end_line, end_column: d.end_column });
    setDiagnosticsOpen(false);
  }

  // Refs for splitter max-width clamping
  let mainRef: HTMLDivElement | undefined;
  let sidePanelRef: HTMLDivElement | undefined;
  let sidePanelObserver: ResizeObserver | undefined;

  // Reactively update window title based on active file and eval status
  createEffect(() => {
    const activeFile = editorStore.state.activeFile;
    const phase = engineStore.state.evalStatus.phase;

    if (!activeFile) {
      document.title = 'Reify';
      return;
    }

    const basename = activeFile.split('/').pop() ?? activeFile;
    if (phase === 'idle') {
      document.title = `${basename} - Reify`;
    } else {
      document.title = `${basename} [${phase}] - Reify`;
    }
  });

  /**
   * Remove a single path from the App-level changedFiles Set.
   * Called whenever a conflict is resolved (reload or overwrite) or when a
   * non-dirty file is silently auto-reloaded.  Keeps the "N files changed"
   * banner in sync with the store-level dirty/externallyChanged state.
   */
  function removeFromChangedFiles(path: string) {
    setChangedFiles((prev) => {
      const next = new Set(prev);
      next.delete(path);
      return next;
    });
  }

  /**
   * Show a conflict prompt (instead of a dead-end error toast) when saving a
   * file that has been modified on disk since it was loaded.  Two actions are
   * offered:
   *   - "Reload from disk" — discard the buffer and reload from disk.
   *   - "Overwrite" — save the buffer as-is, clobbering the newer disk content.
   * The toast's close-X serves as "Cancel" (both actions and the close-X call
   * onDismiss, satisfying the Toast.actions contract).
   *
   * Deduplicated: if a conflict toast is already visible (e.g. the user pressed
   * Ctrl+S multiple times), subsequent calls are ignored.  A stacked second
   * prompt with its own Reload/Overwrite buttons could silently discard newer
   * edits if the user clicks Reload in the older copy after typing more.
   */
  function showSaveConflictPrompt(file: FileData) {
    // Dedup check: bail if a conflict toast with this message is already mounted.
    if (toasts().some((t) => t.message === EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG)) {
      return;
    }
    showToast(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG, 'error', [
      {
        label: SAVE_CONFLICT_RELOAD_LABEL,
        onClick: () => reloadFromDisk(file.path),
      },
      {
        label: SAVE_CONFLICT_OVERWRITE_LABEL,
        onClick: () => overwriteFile(file),
      },
    ]);
  }

  /**
   * Reload a file from disk, replacing the buffer with the current on-disk content.
   * Called by the Reload action in the save conflict prompt.
   * Mirrors the per-file reload logic in handleReload (without the dirty-overlap check
   * since the user has explicitly chosen to discard the buffer).
   */
  async function reloadFromDisk(path: string) {
    try {
      const fileData = await bridgeOpenFile(path);
      editorStore.updateFileContent(fileData.path, fileData.content);
      editorStore.markClean(fileData.path);
      // Remove from changedFiles so the "N files changed" banner disappears.
      removeFromChangedFiles(fileData.path);
    } catch (err) {
      showToast(`Reload failed: ${errorMessage(err)}`, 'error');
    }
  }

  /**
   * Save the buffer as-is, overwriting the newer on-disk content.
   * Called by the Overwrite action in the save conflict prompt.
   */
  async function overwriteFile(file: FileData) {
    try {
      await bridgeSaveFile(file.path, file.content);
      editorStore.markClean(file.path);
      // Remove from changedFiles so the "N files changed" banner disappears.
      removeFromChangedFiles(file.path);
    } catch (err) {
      showToast(`Save failed: ${errorMessage(err)}`, 'error');
    }
  }

  async function handleSave() {
    const activeFile = editorStore.state.activeFile;
    if (!activeFile) return;
    const result = editorStore.canSave(activeFile);
    if (!result.ok) {
      // Exhaustive switch mirrors the identical guard in Editor.tsx (the other
      // Ctrl+S call site): adding a new SaveBlockedReason member produces a
      // TypeScript compile error at BOTH sites, preventing silent policy drift.
      switch (result.reason) {
        case 'not-found':
          // Invariant breach — activeFile should always be in openFiles.  Do
          // not surface a toast since this is not an actionable user condition.
          console.error('Save aborted: active file is not in openFiles', activeFile);
          return;
        case 'externally-changed': {
          // Show a conflict prompt with Reload / Overwrite actions so the user
          // has a clear recovery path instead of a dead-end error toast.
          const file = editorStore.state.openFiles.find((f) => f.path === activeFile);
          if (file) showSaveConflictPrompt(file);
          return;
        }
        default: {
          const _exhaustive: never = result.reason;
          console.error('Save aborted: unhandled save-blocked reason', _exhaustive);
          return;
        }
      }
    }
    try {
      await bridgeSaveFile(result.file.path, result.file.content);
      editorStore.markClean(result.file.path);
    } catch (err) {
      showToast(`Save failed: ${errorMessage(err)}`, 'error');
    }
  }

  /**
   * Shared post-open load sequence: reads file content + engine state into
   * their stores and restores persisted view state atomically.
   *
   * Called from both handleOpen (after picking a path) and handleNew (after
   * writing the starter template).  Keeping this in one place ensures the
   * batch() ordering invariant is never accidentally diverged between the two
   * callers — the subtle race it prevents is documented in the batch() block
   * below.
   */
  async function loadPathIntoStores(path: string): Promise<void> {
    const fileData = await bridgeOpenFile(path);
    editorStore.openFile(fileData);
    // Load into engine for evaluation (meshes, values, constraints)
    const guiState = await bridgeOpenFileEngine(path);
    engineStore.initFromState(guiState);

    // Load persisted view state (sidecar > localStorage > null).
    // Apply BEFORE the entity tree triggers regenerateAutoViews so persisted
    // user views are in place when auto views are generated.
    const persisted = await loadPersistedViews(path);
    // Wrap the three store writes in a single batch so the debounced-save
    // effect observes the path transition AND the new view/camera state
    // atomically.  Without batch(), a future refactor that reorders or
    // interleaves these updates could cause the effect to schedule a write
    // with (oldPath, newState) — which the path-transition branch would
    // then flush into the OUTGOING file's localStorage key, silently
    // cross-corrupting sidecars.  Setting currentFilePath first inside the
    // batch is the critical ordering: the effect sees `path !== activePath`
    // and swaps the saver before any schedule() runs against the new state.
    batch(() => {
      setCurrentFilePath(path);
      if (persisted !== null) {
        viewStateStore.applyPersistedState(persisted);
        // Restore per-viewport camera positions
        for (const [id, camera] of Object.entries(persisted.viewportCameras)) {
          viewportStore.updateCamera(id, camera);
        }
      }
    });
  }

  // Guard for File→New and File→Open: returns true when it is safe to proceed.
  // We check ALL dirty files rather than just the active tab because loadPathIntoStores
  // replaces the full engine state (initFromState), view state, and current path — any
  // open buffer with unsaved edits is effectively unreachable after the switch.
  // TODO(ux): replace window.confirm with a Tauri async dialog (bridge.ask / custom
  //   modal) once the rest of the confirmation UI migrates away from native prompts.
  function confirmDiscardIfDirty(): boolean {
    if (editorStore.state.dirtyFiles.length === 0) return true;
    return window.confirm('You have unsaved changes. Discard them?');
  }

  async function handleOpen() {
    if (!confirmDiscardIfDirty()) return;
    try {
      const path = await pickOpenPath();
      if (!path) return;
      await loadPathIntoStores(path);
    } catch (err) {
      const msg = errorMessage(err);
      console.error('Open file failed:', msg);
      showToast(`Open file failed: ${msg}`, 'error');
    }
  }

  async function handleNew() {
    if (!confirmDiscardIfDirty()) return;
    try {
      const path = await pickSavePath('untitled.ri', 'ri');
      if (!path) return;
      await bridgeSaveFile(path, NEW_FILE_TEMPLATE);
      // Partial-failure note: if bridgeSaveFile succeeds but loadPathIntoStores
      // throws (e.g. the engine rejects the new file), the stub .ri template is
      // left on disk at the user-chosen path.  This is intentional — the file is
      // a valid .ri starting point and the error toast lets the user open it
      // manually via File→Open once the underlying issue is resolved.
      await loadPathIntoStores(path);
    } catch (err) {
      const msg = errorMessage(err);
      console.error('New file failed:', msg);
      showToast(`New file failed: ${msg}`, 'error');
    }
  }

  /**
   * Save the current view state to the sidecar file (.ri.views.json).
   * Called when the user clicks "Save views" in the ViewSelector dropdown.
   * Shows a success or error toast based on the outcome.
   */
  async function handleSaveViews() {
    const path = currentFilePath();
    if (!path) return;

    const viewportCameras: Record<string, CameraState> = {};
    for (const [id, vp] of Object.entries(viewportStore.state.viewports)) {
      if (vp.camera) viewportCameras[id] = vp.camera;
    }
    const composed: PersistentViewState = {
      ...viewStateStore.serializePersistedState(),
      viewportCameras,
      timestamp: new Date().toISOString(),
    };

    try {
      await saveSidecar(path, composed);
      const filename = path.split('/').pop() ?? path;
      showToast(`Views saved to ${filename}.views.json`, 'success');
    } catch (err) {
      showToast(`Failed to save views: ${errorMessage(err)}`, 'error');
    }
  }

  function handleReEvaluate() {
    // Re-evaluate the active file
    const activeFile = editorStore.state.activeFile;
    if (activeFile) {
      const file = editorStore.state.openFiles.find((f) => f.path === activeFile);
      if (file) {
        bridgeUpdateSource(file.path, file.content).catch((err) =>
          showToast(`Re-evaluation failed: ${errorMessage(err)}`, 'error'),
        );
      }
    }
  }

  // Keyboard shortcuts
  useKeyboardShortcuts({
    onNew: handleNew,
    onOpen: handleOpen,
    onSave: handleSave,
    onReEvaluate: handleReEvaluate,
    onExportDialog: () => {
      setShowExportDialog((v) => !v);
    },
    onHelp: () => {
      setShowHelp((v) => !v);
    },
    onToggleChatPanel: handleToggleChat,
    onReloadShortcut: () => {
      if (changedFiles().size > 0) {
        handleReload();
      }
    },
    onDismissReload: () => {
      if (changedFiles().size > 0) {
        handleDismissReload();
      }
    },
    onClearSelection: () => selectionStore.clearSelection(),
    onSwitchViewByIndex: (i: number) => {
      // Delegate ordering to the store's single source of truth so that the
      // number-key dispatch always matches what ViewSelector renders.
      const target = viewStateStore.getOrderedViewIds()[i];
      if (target) viewStateStore.switchView(target);
    },
  });

  let alive = true;
  let unsub: (() => void) | undefined;
  let fileChangedUnsub: (() => void) | undefined;
  let fileRemovedUnsub: (() => void) | undefined;
  let serializationErrorUnsub: (() => void) | undefined;
  let focusEntityUnsub: (() => void) | undefined;
  let navigateToSourceUnsub: (() => void) | undefined;
  let claudeEventUnsub: (() => void) | undefined;
  let sidecarCrashedUnsub: (() => void) | undefined;
  let debugBridgeUnsub: (() => void) | undefined;
  let kernelStatusUnsub: (() => void) | undefined;

  async function initApp() {
    // Clean up existing subscriptions before proceeding (defensive against
    // concurrent or re-entrant initApp calls, e.g. rapid retry)
    unsub?.();
    unsub = undefined;
    fileChangedUnsub?.();
    fileChangedUnsub = undefined;
    fileRemovedUnsub?.();
    fileRemovedUnsub = undefined;
    serializationErrorUnsub?.();
    serializationErrorUnsub = undefined;
    focusEntityUnsub?.();
    focusEntityUnsub = undefined;
    navigateToSourceUnsub?.();
    navigateToSourceUnsub = undefined;
    claudeEventUnsub?.();
    claudeEventUnsub = undefined;
    sidecarCrashedUnsub?.();
    sidecarCrashedUnsub = undefined;
    kernelStatusUnsub?.();
    kernelStatusUnsub = undefined;

    setInitPhase('loading');

    try {
      const initialState = await getInitialState();
      if (!alive) return;
      engineStore.initFromState(initialState);
      for (const file of initialState.files) {
        editorStore.openFile(file);
      }
    } catch (err) {
      console.error('getInitialState failed:', err);
      setInitPhase('error');
      return;
    }

    if (!alive) return;

    // Subscribe to events before showing ready state — "ready" means
    // fully initialized including live update subscriptions
    try {
      const u = await engineStore.subscribeToEvents();
      if (!alive) {
        u();
        return;
      }
      unsub = u;
    } catch (err) {
      showToast('Event subscription failed — some updates may not appear', 'error');
    }

    // Subscribe to file-changed events
    try {
      const unlistenFileChanged = await onFileChanged((data: FileData) => {
        // Only act when the file is currently open.
        // isSameFile handles file:// URI vs bare-path mismatches that the bridge
        // layer emits inconsistently (LSP uses URIs, Tauri commands use bare paths).
        // We pass match.path (the tab's key) to store mutations so the store
        // key stays stable — never accidentally renamed to the URI form.
        const match = editorStore.state.openFiles.find((f) => isSameFile(f.path, data.path));
        if (!match) return;

        if (editorStore.state.dirtyFiles.includes(match.path)) {
          // Dirty path: user has unsaved edits — surface the conflict so they
          // can choose between Reload or Overwrite.
          setChangedFiles((prev) => new Set([...prev, match.path]));
          editorStore.markExternallyChanged(match.path);
        } else {
          // Non-dirty path: no local edits to protect — silently update the
          // buffer so the view stays in sync with disk without any prompt.
          editorStore.updateFileContent(match.path, data.content);
          editorStore.markClean(match.path);
          // Also clear this path from changedFiles in case a stale entry survived
          // (e.g. a previous markExternallyChanged that was then resolved outside
          // the normal conflict flow). Keeps the banner from outliving the conflict.
          removeFromChangedFiles(match.path);
        }
      });
      if (!alive) {
        unlistenFileChanged();
        return;
      }
      fileChangedUnsub = unlistenFileChanged;
    } catch (_err) {
      showToast('File change monitoring unavailable — external edits may not be detected', 'error');
    }

    // Subscribe to file-removed events
    try {
      const unlistenFileRemoved = await onFileRemoved((data) => {
        // Only act when the file is currently open — ignore deletions for files
        // we don't have a tab for (avoids spurious missingFiles entries).
        const isOpen = editorStore.state.openFiles.some((f) => isSameFile(f.path, data.path));
        if (!isOpen) return;
        editorStore.markMissing(data.path);
      });
      if (!alive) {
        unlistenFileRemoved();
        return;
      }
      fileRemovedUnsub = unlistenFileRemoved;
    } catch (_err) {
      showToast('File removal monitoring unavailable — deleted files may not be indicated', 'error');
    }

    // Subscribe to serialization error events
    try {
      const unlistenSerializationError = await onSerializationError((data) => {
        serializationErrorCoalescer.add(data);
      });
      if (!alive) {
        unlistenSerializationError();
        return;
      }
      serializationErrorUnsub = unlistenSerializationError;
    } catch (_err) {
      showToast('Serialization error monitoring unavailable', 'error');
    }

    // Subscribe to focus-entity events (from focus_entity Tauri command and MCP tool).
    //
    // OWNERSHIP: This handler is the sole terminal dispatcher for focus navigation
    // regardless of origin:
    //   • MCP-originated: Claude calls reify_focus_entity → backend emits the event →
    //     this listener fires. No other handler runs.
    //   • User-initiated: handleGroupDoubleClick → navigateToEntity → bridgeFocusEntity
    //     (Tauri command) → backend emits the event → this listener fires.
    //     navigateToEntity's only side effect is triggering the backend command;
    //     flyToEntity and selectEntity run exclusively here.
    try {
      const unlisten = await onFocusEntity((entity) => {
        flyToEntityFn?.(entity);
        selectionStore.selectEntity(entity);
      });
      if (!alive) {
        unlisten();
        return;
      }
      focusEntityUnsub = unlisten;
    } catch (_err) {
      console.warn('Failed to subscribe to focus-entity events:', _err);
    }

    // Fetch initial kernel status. Wrapped in try/catch (not .catch) so that if
    // the bridge import is mocked without getKernelStatus, the synchronous
    // `undefined()` TypeError is captured rather than escaping the async context.
    try {
      const status = await getKernelStatus();
      if (!alive) return;
      engineStore.setKernelStatus(status);
    } catch (err) {
      console.warn('[kernel-status] fetch failed:', err);
    }

    // Subscribe to kernel-status events (so late-binding kernel-availability
    // changes — e.g. future dynamic dylib loading — propagate to the banner).
    try {
      const unlisten = await onKernelStatus((s) => engineStore.setKernelStatus(s));
      if (!alive) {
        unlisten();
        return;
      }
      kernelStatusUnsub = unlisten;
    } catch (_err) {
      console.warn('Failed to subscribe to kernel-status events:', _err);
    }

    // Subscribe to navigate-to-source events (from MCP navigate_to_source tool)
    try {
      const unlisten = await onNavigateToSource(({ file, line, column, end_line, end_column }) => {
        setScrollToLocation({ file_path: file, line, column, end_line, end_column });
      });
      if (!alive) {
        unlisten();
        return;
      }
      navigateToSourceUnsub = unlisten;
    } catch (_err) {
      console.warn('Failed to subscribe to navigate-to-source events:', _err);
    }

    // Subscribe to Claude sidecar events
    try {
      const unlistenClaude = await subscribeToClaudeEvents(claudeStore.handleOutboundMessage);
      if (!alive) {
        unlistenClaude();
        return;
      }
      claudeEventUnsub = unlistenClaude;
    } catch (err) {
      console.error('[claude] subscribeToClaudeEvents failed:', err);
      showToast('Claude assistant unavailable — chat features may not work', 'error');
    }

    // Subscribe to sidecar-crashed events (unexpected process exit)
    try {
      const unlistenSidecarCrashed = await subscribeToSidecarCrashed((reason) =>
        claudeStore.handleSidecarCrashed(reason),
      );
      if (!alive) {
        unlistenSidecarCrashed();
        return;
      }
      sidecarCrashedUnsub = unlistenSidecarCrashed;
    } catch (err) {
      console.error('[claude] subscribeToSidecarCrashed failed:', err);
    }

    // Initialize debug bridge if REIFY_DEBUG=1 (dynamic import for tree-shaking)
    try {
      if (await isDebugEnabled()) {
        setDebugEnabled(true);
        const { initDebugBridge } = await import('./debug');
        const unsub = await initDebugBridge({
          engine: engineStore,
          editor: editorStore,
          selection: selectionStore,
          claude: claudeStore,
        });
        if (!alive) {
          unsub();
          return;
        }
        debugBridgeUnsub = unsub;
      }
    } catch (err) {
      console.error('[debug-bridge] init failed:', err);
    }

    if (!alive) return;
    setInitPhase('ready');
  }

  onMount(() => {
    applyTheme();
    initApp();
  });

  // Side-panel container clamp: once initPhase reaches 'ready' the side panel
  // is rendered and `sidePanelRef` is bound, so we run an initial clamp and
  // attach a ResizeObserver to re-clamp on window/container resize. The ref
  // is undefined while initPhase is 'loading' (Show gate above).
  createEffect(() => {
    if (initPhase() !== 'ready') return;
    if (!sidePanelRef || sidePanelObserver) return;
    clampToContainer();
    if (typeof ResizeObserver !== 'undefined') {
      sidePanelObserver = new ResizeObserver(() => clampToContainer());
      sidePanelObserver.observe(sidePanelRef);
    }
  });

  onCleanup(() => {
    alive = false;
    unsub?.();
    fileChangedUnsub?.();
    fileRemovedUnsub?.();
    serializationErrorUnsub?.();
    focusEntityUnsub?.();
    navigateToSourceUnsub?.();
    serializationErrorCoalescer.cleanup();
    claudeEventUnsub?.();
    sidecarCrashedUnsub?.();
    debugBridgeUnsub?.();
    kernelStatusUnsub?.();
    sidePanelObserver?.disconnect();
    delete window.__REIFY_DEBUG__;
  });

  function handleSetParameter(cellId: string, value: string) {
    bridgeSetParameter(cellId, value).catch((err) =>
      showToast(`Parameter update failed: ${errorMessage(err)}`, 'error'),
    );
  }

  function handleExport() {
    setShowExportDialog(true);
  }

  async function handleDoExport(format: ExportFormat) {
    const defaultName = `export.${format}`;

    let chosenPath: string | null;
    try {
      chosenPath = await pickSavePath(defaultName, format);
    } catch (err) {
      showToast(`Could not open save dialog: ${errorMessage(err)}`, 'error');
      return;
    }

    if (!chosenPath) {
      // User cancelled the file picker — stay on dialog
      return;
    }

    setExporting(true);
    try {
      await bridgeExportGeometry(format, chosenPath);
      showToast(`Exported successfully as ${format.toUpperCase()}`, 'success');
    } catch (err) {
      showToast(`Export failed: ${errorMessage(err)}`, 'error');
    } finally {
      setExporting(false);
      setShowExportDialog(false);
    }
  }

  function handleFitToView() {
    fitToViewFn?.();
  }

  function handleReload() {
    const files = changedFiles();
    if (files.size === 0) return;

    // Check if any changed files have unsaved edits
    const dirtyOverlap = Array.from(files).some((f) =>
      editorStore.state.dirtyFiles.includes(f),
    );

    if (dirtyOverlap && !confirmReload()) {
      setConfirmReload(true);
      return; // Show warning, don't reload yet
    }

    const filePaths = Array.from(files);
    const promises = filePaths.map((path) =>
      bridgeOpenFile(path)
        .then((fileData) => {
          editorStore.updateFileContent(fileData.path, fileData.content);
          return path;
        }),
    );
    Promise.allSettled(promises)
      .then((results) => {
        const succeededPaths: string[] = [];
        const failedPaths: string[] = [];
        for (let i = 0; i < results.length; i++) {
          if (results[i].status === 'fulfilled') {
            succeededPaths.push(filePaths[i]);
          } else {
            failedPaths.push(filePaths[i]);
          }
        }
        // Functional update: only delete succeeded paths, preserving any
        // concurrently-added paths from onFileChanged events during reload
        setChangedFiles((prev) => {
          const next = new Set(prev);
          for (const path of succeededPaths) {
            next.delete(path);
          }
          return next;
        });
        for (const path of succeededPaths) {
          // markClean clears both dirtyFiles and externallyChanged: after a
          // reload the buffer is replaced with disk content, so unsaved user
          // edits are gone and the disk-divergence flag must be cleared too.
          editorStore.markClean(path);
        }
        if (failedPaths.length > 0) {
          const count = failedPaths.length;
          showToast(
            `${count} file${count > 1 ? 's' : ''} failed to reload`,
            'error',
          );
        }
        setConfirmReload(false);
      });
  }

  function handleDismissReload() {
    setChangedFiles(new Set<string>());
    editorStore.clearAllExternallyChanged();
    setConfirmReload(false);
  }

  function handleRetry() {
    initApp();
  }

  function handleDismissToast(id: string) {
    // If this toast was a fuzzy-rebind prompt, release the pair from the
    // "currently-shown" guard so a legitimate later tree change can surface
    // the prompt again (unless the user already added it to ignoredPairs).
    const pairKey = rebindToastPairs.get(id);
    if (pairKey !== undefined) {
      rebindToastPairs.delete(id);
      rebindShownPairs.delete(pairKey);
    }
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }

  function handleFileClick(path: string) {
    editorStore.setActiveFile(path);
  }

  function handleLeftResize(delta: number) {
    const cw = mainRef?.clientWidth ?? 0;
    const maxWidth = cw > 0 ? cw - sideWidth() - MIN_PANEL_WIDTH - 8 : Infinity;
    setEditorWidth((w) => Math.min(maxWidth, Math.max(MIN_PANEL_WIDTH, w + delta)));
  }

  function handleRightResize(delta: number) {
    const cw = mainRef?.clientWidth ?? 0;
    const maxWidth = cw > 0 ? cw - editorWidth() - MIN_PANEL_WIDTH - 8 : Infinity;
    setSideWidth((w) => Math.min(maxWidth, Math.max(MIN_PANEL_WIDTH, w - delta)));
  }

  // Re-flow side-panel sub-panel heights so they fit `sidePanelRef.clientHeight`.
  // Called on mount, on container resize (ResizeObserver), and when chat is
  // toggled open. The drag handlers below have their own per-drag clamp via
  // `reservedForOthers`; this covers the cases where the user can't drag —
  // first paint with oversized persisted heights, and window/container shrink.
  function clampToContainer(): void {
    if (!sidePanelRef) return;
    const ch = sidePanelRef.clientHeight;
    if (ch <= 0) return;
    const clamped = clampPanelHeightsToFit(
      {
        designTree: designTreeHeight(),
        property: propertyHeight(),
        constraint: constraintHeight(),
      },
      ch,
      {
        chatOpen: chatOpen(),
        chatMinHeight: CHAT_MIN_HEIGHT,
        minPanelHeight: MIN_PANEL_HEIGHT,
        splitterThickness: SPLITTER_THICKNESS,
      },
    );
    if (
      clamped.designTree !== designTreeHeight() ||
      clamped.property !== propertyHeight() ||
      clamped.constraint !== constraintHeight()
    ) {
      batch(() => {
        setDesignTreeHeight(clamped.designTree);
        setPropertyHeight(clamped.property);
        setConstraintHeight(clamped.constraint);
      });
    }
  }

  // Total pixels reserved by sibling panels + splitters when resizing one sub-panel.
  // Three splitters when chat is open, two when closed. The chat-open case reserves
  // CHAT_MIN_HEIGHT so chat can never be silently hidden.
  function reservedForOthers(currentSignal: 'designTree' | 'property' | 'constraint'): number {
    const splitters = (chatOpen() ? 3 : 2) * SPLITTER_THICKNESS;
    const chatFloor = chatOpen() ? CHAT_MIN_HEIGHT : 0;
    const designTree = currentSignal === 'designTree' ? 0 : designTreeHeight();
    const property = currentSignal === 'property' ? 0 : propertyHeight();
    const constraint = currentSignal === 'constraint' ? 0 : constraintHeight();
    return designTree + property + constraint + chatFloor + splitters;
  }

  function handleDesignTreeResize(delta: number) {
    const ch = sidePanelRef?.clientHeight ?? 0;
    const maxHeight = ch > 0 ? ch - reservedForOthers('designTree') : Infinity;
    setDesignTreeHeight((h) => Math.min(maxHeight, Math.max(MIN_PANEL_HEIGHT, h + delta)));
  }

  function handleSideResize(delta: number) {
    const ch = sidePanelRef?.clientHeight ?? 0;
    const maxHeight = ch > 0 ? ch - reservedForOthers('property') : Infinity;
    setPropertyHeight((h) => Math.min(maxHeight, Math.max(MIN_PANEL_HEIGHT, h + delta)));
  }

  function handleConstraintResize(delta: number) {
    const ch = sidePanelRef?.clientHeight ?? 0;
    const maxHeight = ch > 0 ? ch - reservedForOthers('constraint') : Infinity;
    setConstraintHeight((h) => Math.min(maxHeight, Math.max(MIN_PANEL_HEIGHT, h + delta)));
  }

  function handleViewportSelect(entityPath: string | null, modifiers?: { ctrl: boolean; shift: boolean }) {
    if (!entityPath) {
      selectionStore.selectEntity(null);
      return;
    }
    // Ctrl+click: toggle multi-selection without navigating to source
    if (modifiers?.ctrl) {
      selectionStore.toggleSelect(entityPath);
      return;
    }
    // Plain click or shift+click: navigate to source and single-select
    navigateToSource(entityPath, {
      getSourceLocation: bridgeGetSourceLocation,
      scrollEditor: (loc) => setScrollToLocation(loc),
      selectEntity: (ep) => selectionStore.selectEntity(ep),
    });
  }

  function handleDesignTreeSelect(path: string, mods: { ctrl: boolean; shift: boolean }) {
    if (mods.ctrl) {
      selectionStore.toggleSelect(path);
      return;
    }
    if (mods.shift) {
      selectionStore.rangeSelect([path]);
      return;
    }
    handleViewportSelect(path, mods);
  }

  function handleGroupDoubleClick(groupName: string) {
    navigateToEntity(groupName, {
      focusEntity: bridgeFocusEntity,
    });
  }

  function handleConstraintSelect(constraint: ConstraintData) {
    const valuesArray = Object.values(engineStore.state.values);
    navigateFromConstraint(constraint, valuesArray, {
      selectEntity: (ep) => selectionStore.selectEntity(ep),
      setHighlightedParams: (ids) => selectionStore.setHighlightedParams(ids),
    });
  }

  function handleAskClaude(context: string) {
    // Open chat panel if closed, and pre-populate with context
    setChatOpen(true);
    // Send context as a message to Claude
    claudeStore.sendMessage(context, {});
  }

  function handleToggleChat() {
    setChatOpen((v) => !v);
    // Opening chat reserves an additional splitter (4 px) + chat floor
    // (160 px); without re-clamping, persisted panel heights can push the
    // chat panel below the viewport on small windows. Closing is a no-op
    // for the clamp (`sum <= available` short-circuits).
    clampToContainer();
  }

  return (
    <>
      <Show when={showHelp()}>
        <KeyboardHelp onClose={() => setShowHelp(false)} />
      </Show>
      <Show when={initPhase() === 'loading'}>
        <div data-testid="app-loading" class={styles.loading}>
          <div class={styles.spinner} />
          <p>Loading...</p>
        </div>
      </Show>
      <Show when={initPhase() === 'error'}>
        <div data-testid="app-error" class={styles.errorState}>
          <p>Failed to load application state.</p>
          <button onClick={handleRetry} disabled={initPhase() === 'loading'}>Retry</button>
        </div>
      </Show>
      <Show when={initPhase() === 'ready'}>
        <div data-testid="app-layout" class={styles.layout}>
          <Show when={engineStore.state.kernelStatus && !engineStore.state.kernelStatus.available}>
            <div data-testid="kernel-degraded-banner" class={styles.kernelBanner} role="alert">
              {engineStore.state.kernelStatus!.message}
            </div>
          </Show>
          <MenuBar
            onNew={handleNew}
            onOpen={handleOpen}
            onSave={handleSave}
            onExport={handleExport}
            onReEvaluate={handleReEvaluate}
            onFitToView={handleFitToView}
            onToggleChat={handleToggleChat}
            onHelp={() => setShowHelp((v) => !v)}
          />
          <Toolbar onExport={handleExport} onFitToView={handleFitToView} />
          <ReloadPrompt
            filePaths={Array.from(changedFiles())}
            hasDirtyFiles={confirmReload()}
            onReload={handleReload}
            onDismiss={handleDismissReload}
          />
          <div
            ref={mainRef}
            class={styles.main}
            style={{ 'grid-template-columns': `${editorWidth()}px 4px 1fr 4px ${sideWidth()}px` }}
          >
            <div data-testid="editor-panel" class={styles.editorPanel}>
              <FileBrowser
                files={editorStore.state.openFiles}
                activeFile={editorStore.state.activeFile}
                onFileClick={handleFileClick}
              />
              <FileTabs store={editorStore} />
              <Editor store={editorStore} scrollToLocation={scrollToLocation} onOpen={handleOpen} onError={(msg) => showToast(msg, 'error')} onSaveConflict={(file) => showSaveConflictPrompt(file)} />
            </div>
            <Splitter orientation="vertical" onResize={handleLeftResize} data-testid="splitter-left" />
            <div data-testid="viewport-panel" class={styles.viewportPanel}>
              <DualViewport
                engineStore={engineStore}
                defPreviewStore={defPreviewStore}
                viewportStore={viewportStore}
                defPreviewActive={defPreviewActivation.defPreviewActive}
                designViewportActive={hasMeshes}
                defName={() => defPreviewStore.state.defName}
                onForceExpand={(id) => viewportStore.setForceExpanded(id, true)}
                onSelect={handleViewportSelect}
                onHover={(path) => selectionStore.hoverEntity(path)}
                selectedEntity={selectionStore.state.selectedEntity}
                selectedEntities={selectionStore.state.selectedEntities}
                hoveredEntity={selectionStore.state.hoveredEntity}
                evalStatus={engineStore.state.evalStatus}
                flyToEntityRef={(fn) => { flyToEntityFn = fn; }}
                fitToViewRef={(fn) => { fitToViewFn = fn; }}
                entityVisibility={effectiveVisibility()}
              />
            </div>
            <Splitter orientation="vertical" onResize={handleRightResize} data-testid="splitter-right" />
            <div
              ref={sidePanelRef}
              data-testid="side-panel"
              class={styles.sidePanel}
              style={{ 'grid-template-rows': (() => {
                const hasMech = mechanismStore.state.descriptors.length > 0;
                const hasAR = engineStore.state.autoResolve.active;
                const hasChat = chatOpen();
                const base = `${designTreeHeight()}px 4px ${propertyHeight()}px 4px`;
                // Middle tracks: one `auto` per optional panel present (autoResolve then
                // mechanism). No splitter between cons and the first optional panel — adding
                // one would shift subsequent children up a track and collapse chat into 4px.
                const midTracks = [
                  ...(hasAR ? ['auto'] : []),
                  ...(hasMech ? ['auto'] : []),
                ];
                const midStr = midTracks.length > 0
                  ? `${constraintHeight()}px ${midTracks.join(' ')}`
                  : null;
                if (hasChat) {
                  return `${base} ${midStr ?? `${constraintHeight()}px`} 4px minmax(${CHAT_MIN_HEIGHT}px, 1fr)`;
                }
                return midStr
                  ? `${base} ${midStr}`
                  : `${base} 1fr`;
              })() }}
            >
              <DesignTree
                tree={entityTree()}
                viewStateStore={viewStateStore}
                selectedEntity={selectionStore.state.selectedEntity}
                selectedEntities={selectionStore.state.selectedEntities}
                anchorEntity={selectionStore.state.anchorEntity}
                onSelect={handleDesignTreeSelect}
                onRangeSelect={selectionStore.rangeSelect}
                onSelectAll={selectionStore.selectAll}
                onOpenManage={() => setViewManageOpen(true)}
                onSaveViews={handleSaveViews}
              />
              <Splitter orientation="horizontal" onResize={handleDesignTreeResize} data-testid="splitter-design-tree" />
              <PropertyEditor
                values={engineStore.state.values}
                selectedEntity={selectionStore.state.selectedEntity}
                onSetParameter={handleSetParameter}
                onGroupDoubleClick={handleGroupDoubleClick}
                highlightedParams={selectionStore.state.highlightedParams}
              />
              <Splitter orientation="horizontal" onResize={handleSideResize} data-testid="splitter-side" />
              <ConstraintPanel
                constraints={engineStore.state.constraints}
                values={engineStore.state.values}
                onConstraintSelect={handleConstraintSelect}
                onAskClaude={handleAskClaude}
              />
              {/* AutoResolvePanel: auto-promotes when a param=auto loop is active,
                  auto-restores (unmounts) when complete — no bookkeeping needed. */}
              <Show when={engineStore.state.autoResolve.active}>
                <AutoResolvePanel state={engineStore.state.autoResolve} />
              </Show>
              {/* SolverProgressOverlay: shows after >1s of in-flight CG solver
                  progress ticks; hidden (debounce) for sub-second solves. */}
              <Show when={engineStore.state.solverProgress.visible}>
                <SolverProgressOverlay
                  progress={engineStore.state.solverProgress.latest}
                  trace={engineStore.state.solverProgress.trace}
                  coarseReached={engineStore.state.solverProgress.coarseReached}
                  onCancel={engineStore.cancelSolve}
                />
              </Show>
              {/* WarmPoolDebugPanel: REIFY_DEBUG=1 only — PRD §11 Q6 resolution */}
              <Show when={debugEnabled()}>
                <WarmPoolDebugPanel />
              </Show>
              <Show when={mechanismStore.state.descriptors.length > 0}>
                <MechanismPanel
                  descriptors={mechanismStore.state.descriptors}
                  onSetParameter={handleSetParameter}
                  onScrubLocal={(cellId, jointIndex, valueSi) =>
                    mechanismStore.setOptimistic(cellId ?? '', jointIndex, valueSi)
                  }
                  getEffectiveValueSi={mechanismStore.getEffectiveValueSi}
                />
              </Show>
              <Show when={chatOpen()}>
                <Splitter orientation="horizontal" onResize={handleConstraintResize} data-testid="splitter-constraint" />
                <ChatPanel
                  store={claudeStore}
                  selectedEntity={selectionStore.state.selectedEntity ?? undefined}
                  engineConstraints={Object.values(engineStore.state.constraints)}
                  activeFile={editorStore.state.activeFile ?? undefined}
                />
              </Show>
            </div>
          </div>
          <StatusBar
            evalStatus={engineStore.state.evalStatus}
            meshes={engineStore.state.meshes}
            constraints={engineStore.state.constraints}
            claudeStatus={claudeStore.state.sessionStatus}
            onToggleChat={handleToggleChat}
            tessellationDiagnostics={engineStore.state.tessellationDiagnostics}
            compileDiagnostics={engineStore.state.compileDiagnostics}
            onToggleDiagnostics={handleToggleDiagnostics}
          />
          <ExportDialog
            open={showExportDialog()}
            exporting={exporting()}
            onExport={handleDoExport}
            onClose={() => setShowExportDialog(false)}
          />
          <DiagnosticsPanel
            open={diagnosticsOpen()}
            diagnostics={allDiagnostics()}
            onClose={() => setDiagnosticsOpen(false)}
            onNavigate={handleNavigateToDiagnostic}
          />
          <ViewManageModal
            open={viewManageOpen()}
            store={viewStateStore}
            onClose={() => setViewManageOpen(false)}
          />
          <div class={styles.toastContainer}>
            <For each={toasts()}>
              {(t) => (
                <Toast
                  message={t.message}
                  type={t.type}
                  onDismiss={() => handleDismissToast(t.id)}
                  actions={t.actions}
                />
              )}
            </For>
          </div>
        </div>
      </Show>
    </>
  );
};

export default App;
