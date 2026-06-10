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
  FindUsesPanel,
  AutoResolvePanel,
  SolverProgressOverlay,
  BucklingPanel,
} from './panels';
import type { DiagnosticEntry } from './panels';
import type { ReferenceResult } from './editor/references';
import { WarmPoolDebugPanel } from './debug/WarmPoolDebugPanel';
import { Splitter } from './components/Splitter';
import { KeyboardHelp } from './components/KeyboardHelp';
import { CommandPalette } from './components/CommandPalette';
import { useKeyboardShortcuts, paletteCommands, runCommand } from './hooks/useKeyboardShortcuts';
import { createLspClient } from './editor/lspClient';
import { createEngineStore } from './stores/engineStore';
import { createEditorStore } from './stores/editorStore';
import { createSelectionStore } from './stores/selectionStore';
import { createClaudeStore } from './stores/claudeStore';
import { createViewStateStore } from './stores/viewStateStore';
import { createLayoutStore } from './stores/layoutStore';
import { createViewportStore, type CameraState } from './stores/viewportStore';
import { createDefPreviewStore } from './stores/defPreviewStore';
import { createMechanismStore } from './stores/mechanismStore';
import { createBucklingStore, subscribeModeShapeFrames } from './stores/bucklingStore';
import { createDefPreviewActivation } from './hooks/useDefPreviewActivation';
import { createEditorSelectionSync, createEditorHoverSync } from './hooks/useEditorSelectionSync';
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
import { clampPanelHeightsToFit, clampProblemsHeight } from './hooks/useLayoutPersistence';
import { createSerializationErrorCoalescer } from './hooks/useSerializationErrorCoalescer';
import { loadSidecar, saveSidecar } from './stores/sidecarPersistence';
import { loadViewPersistence, createDebouncedSaver, type DebouncedSaver } from './stores/viewPersistence';
import { findFuzzyCandidate } from './stores/fuzzyPathMatcher';
import type { PersistentViewState } from './types';
import styles from './App.module.css';

export const NEW_FILE_TEMPLATE = '// New design\n';

/** Minimal structural interface required by {@link navigateToDiagnostic}.
 *  Using a structural interface keeps the function testable without depending
 *  on the concrete createEditorStore return type. */
export interface NavigateToDiagnosticStore {
  state: {
    activeFile: string | null;
    openFiles: Pick<FileData, 'path'>[];
  };
  setActiveFile: (path: string) => void;
  openFile: (file: FileData) => void;
}

/**
 * Navigate the editor to a diagnostic location.
 *
 * Extracted from handleNavigateToDiagnostic so it is dependency-injected and
 * unit-testable without rendering App. Follows the precedent of
 * gotoDefinitionCommand / resolveAndNavigate (task 4206).
 *
 * Logic (implemented incrementally across γ steps):
 *  (1) Refuse span-less diagnostics (has_location === false) — strict === false
 *      so absent/undefined or true keep navigating (α's default-true contract).
 *  (2) Same-file: call setScrollToLocation only.
 *  (3) Already-open cross-file: setActiveFile then setScrollToLocation (step-4).
 *  (4) Not-open cross-file: openFile from disk then setScrollToLocation (step-6).
 *  (5) Open failure: error toast; no scroll (step-8).
 */
export async function navigateToDiagnostic(
  d: DiagnosticEntry,
  deps: {
    store: NavigateToDiagnosticStore;
    openFile: (path: string) => Promise<FileData>;
    setScrollToLocation: (loc: SourceLocation) => void;
    showToast: (message: string, type: ToastMessage['type']) => void;
  },
): Promise<void> {
  // (1) Refuse synthetic span-less diagnostics.
  if (d.has_location === false) return;

  const loc: SourceLocation = {
    file_path: d.file_path,
    line: d.line,
    column: d.column,
    end_line: d.end_line,
    end_column: d.end_column,
  };

  const active = deps.store.state.activeFile;
  if (!(active && isSameFile(d.file_path, active))) {
    // (3) Cross-file: file is already open in a tab → switch to it.
    const open = deps.store.state.openFiles.find((f) => isSameFile(f.path, d.file_path));
    if (open) {
      // (3) Already-open cross-file: just activate.
      deps.store.setActiveFile(open.path);
    } else {
      // (4) Not yet open: read from disk and load into the store.
      const fileData = await deps.openFile(d.file_path);
      deps.store.openFile(fileData);
    }
  }

  // All non-error paths fall through here so the scroll fires AFTER any
  // file-switch, guaranteeing the Editor sees the swapped doc first.
  deps.setScrollToLocation(loc);
}

const MIN_PANEL_WIDTH = 150;
const MIN_PANEL_HEIGHT = 80;
const CHAT_MIN_HEIGHT = 160;
const SPLITTER_THICKNESS = 4;

let toastIdCounter = 0;

const App: Component = () => {
  const editorStore = createEditorStore();
  const selectionStore = createSelectionStore();
  // Epoch counter: incremented once per engine (re)initialization so that any
  // in-flight bridgeGetEntityTree() fetch that was issued before the latest
  // engine load is recognized as stale when its .then resolves and dropped.
  // This is the epoch/staleness guard described in the refreshEntityTree comment.
  let engineLoadEpoch = 0;

  const engineStore = createEngineStore({
    onEntityRemoved: (id) => selectionStore.clearIfRemoved(id),
    onEngineReinitialized: () => {
      // Bump the epoch BEFORE calling refreshEntityTree so the fresh fetch
      // captures the new epoch value; any still-pending pre-reinit fetch
      // will see a mismatch and be dropped.
      engineLoadEpoch += 1;
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
  const bucklingStore = createBucklingStore();

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

  // Deduplicate concurrent in-flight getEntityAtSourceLocation calls for the same
  // (line, col): both createEditorSelectionSync and createEditorHoverSync debounce at
  // 200ms and fire back-to-back after each cursor change. Sharing the same in-flight
  // Promise halves the IPC round-trips — the second hook joins the first Promise
  // rather than issuing a redundant bridge call. Entries are cleared in .finally()
  // so the cache never holds stale results across distinct cursor positions.
  const _pendingEntityResolves = new Map<string, Promise<string | null>>();
  function sharedGetEntityAtSourceLocation(line: number, col: number): Promise<string | null> {
    const key = `${line}:${col}`;
    const inflight = _pendingEntityResolves.get(key);
    if (inflight !== undefined) return inflight;
    const promise = bridgeGetEntityAtSourceLocation(line, col).finally(() => {
      _pendingEntityResolves.delete(key);
    });
    _pendingEntityResolves.set(key, promise);
    return promise;
  }

  // Editor→entity sync: watches editor cursor → debounces 200ms → resolves entity
  // at cursor position → updates selectionStore + flies to entity in viewport.
  // Equality-check guard prevents viewport-click → editor-scroll → cursor-move bounce.
  createEditorSelectionSync({
    editorStore,
    selectionStore,
    getEntityAtSourceLocation: sharedGetEntityAtSourceLocation,
    selectEntity: (ep) => selectionStore.selectEntity(ep),
    flyToEntity: (ep) => flyToEntityFn?.(ep),
    debounceMs: 200,
  });

  // Editor→hover sync: watches editor cursor → debounces 200ms → resolves entity
  // at cursor position → updates selectionStore.hoveredEntity (transient; clears on
  // null cursor or null resolution, unlike the selection hook which preserves selection).
  createEditorHoverSync({
    editorStore,
    getEntityAtSourceLocation: sharedGetEntityAtSourceLocation,
    hoverEntity: (ep) => selectionStore.hoverEntity(ep),
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
    // Epoch/staleness guard: capture the current epoch before the async fetch.
    // If engineLoadEpoch advances while this fetch is in flight (because a
    // newer engine load began — e.g. File→Open during the fetch window), the
    // snapshot we receive predates the current design.  Applying it via
    // reconcileToTree would prune the freshly-loaded design's meshes, blanking
    // the viewport.  Dropping the stale result is safe because the reinit that
    // advanced the epoch already kicked off a fresh refreshEntityTree that will
    // reconcile against the correct tree.
    //
    // The cross-root guard in engineStore.reconcileToTree is a complementary
    // defence-in-depth layer (catches wholly-disjoint snapshots at the store
    // level), but it does not cover partial-overlap snapshots where some entities
    // happen to share paths across designs.  The epoch guard is the surgical fix.
    const epochAtFetch = engineLoadEpoch;
    bridgeGetEntityTree()
      .then((t) => {
        if (!alive) return;
        // Staleness guard: if the epoch advanced since this fetch was issued,
        // the snapshot predates the current design — drop it.
        if (epochAtFetch !== engineLoadEpoch) return;
        setEntityTree(t);
        engineStore.reconcileToTree(t);
      })
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

  const layoutStore = createLayoutStore();

  // Init phase: loading → ready | error
  const [initPhase, setInitPhase] = createSignal<'loading' | 'ready' | 'error'>('loading');

  // Chat panel open/closed state
  const [chatOpen, setChatOpen] = createSignal(true);

  // Export dialog state
  const [showExportDialog, setShowExportDialog] = createSignal(false);

  // View manage modal state
  const [viewManageOpen, setViewManageOpen] = createSignal(false);

  // Diagnostics panel state lives in layoutStore (problemsCollapsed / problemsHeight).
  // Find-uses panel state (Shift+F12 references provider, task 4202 β). Results
  // are held in native LSP (0-based) coordinates; onNavigate converts to the
  // 1-based SourceLocation when driving setScrollToLocation.
  const [findUsesOpen, setFindUsesOpen] = createSignal(false);
  const [findUsesResults, setFindUsesResults] = createSignal<ReferenceResult[]>([]);
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

  // Command palette state
  const [showPalette, setShowPalette] = createSignal(false);
  const [paletteMode, setPaletteMode] = createSignal<'command' | 'symbol'>('command');
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
  // Live-buffer getter: Editor hands App a closure over the active CM view so
  // save/re-evaluate consumers read the current doc rather than the stale store
  // snapshot.  Mirrors the flyToEntityRef/fitToViewRef child→parent handle pattern.
  let getLiveEditorContent: (() => string | null) | undefined;

  // Both the compile badge and the tessellation badge call this handler — toggle
  // problemsCollapsed. Clicking while expanded collapses; clicking while collapsed expands.
  function handleToggleDiagnostics() {
    layoutStore.setProblemsCollapsed((c) => !c);
  }

  function handleNavigateToDiagnostic(d: DiagnosticEntry) {
    // Delegate to the exported, dependency-injected function (γ task-4403).
    // Panel stays open after navigation (docked design — no modal to dismiss).
    void navigateToDiagnostic(d, {
      store: editorStore,
      openFile: bridgeOpenFile,
      setScrollToLocation,
      showToast,
    });
  }

  // Refs for splitter max-width clamping
  let mainRef: HTMLDivElement | undefined;
  let sidePanelRef: HTMLDivElement | undefined;
  let sidePanelObserver: ResizeObserver | undefined;
  let editorPanelRef: HTMLDivElement | undefined;
  let editorPanelObserver: ResizeObserver | undefined;

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
    // Snapshot the live buffer at prompt-CREATION time.  The conflict prompt is
    // an async toast; if the user switches tabs between seeing it and clicking
    // Overwrite, a click-time live read would return the now-active file's
    // content (wrong file).  Snapshotting here captures the buffer as of the
    // Ctrl+S keypress, which is exactly what the user intended to save.
    const liveContent = getLiveEditorContent?.() ?? file.content;
    showToast(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG, 'error', [
      {
        label: SAVE_CONFLICT_RELOAD_LABEL,
        onClick: () => reloadFromDisk(file.path),
      },
      {
        label: SAVE_CONFLICT_OVERWRITE_LABEL,
        onClick: () => overwriteFile(file.path, liveContent),
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
   * Save the supplied content as-is, overwriting the newer on-disk content.
   * Called by the Overwrite action in the save conflict prompt with the live
   * buffer content snapshotted at prompt-creation time.
   */
  async function overwriteFile(path: string, content: string) {
    try {
      await bridgeSaveFile(path, content);
      editorStore.markClean(path);
      // Remove from changedFiles so the "N files changed" banner disappears.
      removeFromChangedFiles(path);
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
      // Read the LIVE buffer rather than the stale store snapshot.
      // Falls back to result.file.content when the handle is not wired
      // (e.g. App tests that mock Editor without wiring liveContentRef).
      const content = getLiveEditorContent?.() ?? result.file.content;
      await bridgeSaveFile(result.file.path, content);
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
    // Re-evaluate the active file using the LIVE buffer content.
    // getLiveEditorContent?.() reads the current CM view document rather than
    // the stale store snapshot (anti-loop invariant: store content is only
    // updated at file-open/reload time, never during typing).  Falls back to
    // file.content when the handle is not wired (e.g. in App tests that don't
    // opt in to the live-content handle).
    const activeFile = editorStore.state.activeFile;
    if (activeFile) {
      const file = editorStore.state.openFiles.find((f) => f.path === activeFile);
      if (file) {
        const content = getLiveEditorContent?.() ?? file.content;
        bridgeUpdateSource(file.path, content).catch((err) =>
          showToast(`Re-evaluation failed: ${errorMessage(err)}`, 'error'),
        );
      }
    }
  }

  // Keyboard shortcuts — shared callback object used by both the global handler
  // and the palette's runCommand() so the two can never drift.
  const shortcutCallbacks = {
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
    onCommandPalette: () => {
      setPaletteMode('command');
      setShowPalette(true);
    },
    onSymbolJump: () => {
      setPaletteMode('symbol');
      setShowPalette(true);
    },
    onToggleDiagnostics: handleToggleDiagnostics,
  };
  useKeyboardShortcuts(shortcutCallbacks);

  // Palette symbol fetch — uses the same URI normalisation as Editor.tsx:104-108
  // so the request URI matches the uri used at didOpen time.
  function pathToUri(filePath: string): string {
    if (filePath.startsWith('file://')) return filePath;
    return `file://${filePath.startsWith('/') ? '' : '/'}${filePath}`;
  }

  async function fetchPaletteSymbols() {
    const f = editorStore.state.activeFile;
    if (!f) return [];
    return createLspClient().documentSymbol(pathToUri(f));
  }

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
  let bucklingFrameUnsub: (() => void) | undefined;

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
    bucklingFrameUnsub?.();
    bucklingFrameUnsub = undefined;

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

    // Subscribe to mode-shape-frame IPC events for the buckling animator
    try {
      const unlistenBuckling = await subscribeModeShapeFrames(bucklingStore);
      if (!alive) {
        unlistenBuckling();
        return;
      }
      bucklingFrameUnsub = unlistenBuckling;
    } catch (err) {
      console.error('[buckling] subscribeModeShapeFrames failed:', err);
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
          viewState: viewStateStore,
          layout: layoutStore,
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

  // Editor-panel container clamp: clamp the problems panel height when the
  // editor column is resized or on first paint with an oversized persisted value.
  // problemsHeight reads are wrapped in untrack() so this function does not
  // create a reactive dependency on problemsHeight — the effect that calls it
  // should only re-subscribe to initPhase(), not to the height it is clamping.
  function clampEditorPanel(): void {
    if (!editorPanelRef) return;
    const ch = editorPanelRef.clientHeight;
    if (ch <= 0) return;
    const currentHeight = untrack(() => layoutStore.state.problemsHeight);
    const clamped = clampProblemsHeight(currentHeight, ch, {
      minPanelHeight: MIN_PANEL_HEIGHT,
      editorMinHeight: MIN_PANEL_HEIGHT,
      splitterThickness: SPLITTER_THICKNESS,
    });
    if (clamped !== currentHeight) {
      layoutStore.setProblemsHeight(clamped);
    }
  }

  createEffect(() => {
    if (initPhase() !== 'ready') return;
    if (!editorPanelRef || editorPanelObserver) return;
    clampEditorPanel();
    if (typeof ResizeObserver !== 'undefined') {
      editorPanelObserver = new ResizeObserver(() => clampEditorPanel());
      editorPanelObserver.observe(editorPanelRef);
    }
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
    bucklingFrameUnsub?.();
    sidePanelObserver?.disconnect();
    editorPanelObserver?.disconnect();
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
    const maxWidth = cw > 0 ? cw - layoutStore.state.sideWidth - MIN_PANEL_WIDTH - 8 : Infinity;
    layoutStore.setEditorWidth((w) => Math.min(maxWidth, Math.max(MIN_PANEL_WIDTH, w + delta)));
  }

  function handleRightResize(delta: number) {
    const cw = mainRef?.clientWidth ?? 0;
    const maxWidth = cw > 0 ? cw - layoutStore.state.editorWidth - MIN_PANEL_WIDTH - 8 : Infinity;
    layoutStore.setSideWidth((w) => Math.min(maxWidth, Math.max(MIN_PANEL_WIDTH, w - delta)));
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
        designTree: layoutStore.state.designTreeHeight,
        property: layoutStore.state.propertyHeight,
        constraint: layoutStore.state.constraintHeight,
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
      clamped.designTree !== layoutStore.state.designTreeHeight ||
      clamped.property !== layoutStore.state.propertyHeight ||
      clamped.constraint !== layoutStore.state.constraintHeight
    ) {
      batch(() => {
        layoutStore.setDesignTreeHeight(clamped.designTree);
        layoutStore.setPropertyHeight(clamped.property);
        layoutStore.setConstraintHeight(clamped.constraint);
      });
    }
  }

  // Total pixels reserved by sibling panels + splitters when resizing one sub-panel.
  // Three splitters when chat is open, two when closed. The chat-open case reserves
  // CHAT_MIN_HEIGHT so chat can never be silently hidden.
  function reservedForOthers(currentSignal: 'designTree' | 'property' | 'constraint'): number {
    const splitters = (chatOpen() ? 3 : 2) * SPLITTER_THICKNESS;
    const chatFloor = chatOpen() ? CHAT_MIN_HEIGHT : 0;
    const designTree = currentSignal === 'designTree' ? 0 : layoutStore.state.designTreeHeight;
    const property = currentSignal === 'property' ? 0 : layoutStore.state.propertyHeight;
    const constraint = currentSignal === 'constraint' ? 0 : layoutStore.state.constraintHeight;
    return designTree + property + constraint + chatFloor + splitters;
  }

  function handleDesignTreeResize(delta: number) {
    const ch = sidePanelRef?.clientHeight ?? 0;
    const maxHeight = ch > 0 ? ch - reservedForOthers('designTree') : Infinity;
    layoutStore.setDesignTreeHeight((h) => Math.min(maxHeight, Math.max(MIN_PANEL_HEIGHT, h + delta)));
  }

  function handleSideResize(delta: number) {
    const ch = sidePanelRef?.clientHeight ?? 0;
    const maxHeight = ch > 0 ? ch - reservedForOthers('property') : Infinity;
    layoutStore.setPropertyHeight((h) => Math.min(maxHeight, Math.max(MIN_PANEL_HEIGHT, h + delta)));
  }

  function handleConstraintResize(delta: number) {
    const ch = sidePanelRef?.clientHeight ?? 0;
    const maxHeight = ch > 0 ? ch - reservedForOthers('constraint') : Infinity;
    layoutStore.setConstraintHeight((h) => Math.min(maxHeight, Math.max(MIN_PANEL_HEIGHT, h + delta)));
  }

  // Problems panel is BELOW its splitter, so dragging down shrinks it: h - delta.
  function handleProblemsResize(delta: number) {
    const ch = editorPanelRef?.clientHeight ?? 0;
    layoutStore.setProblemsHeight((h) =>
      clampProblemsHeight(h - delta, ch, {
        minPanelHeight: MIN_PANEL_HEIGHT,
        editorMinHeight: MIN_PANEL_HEIGHT,
        splitterThickness: SPLITTER_THICKNESS,
      }),
    );
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
    // Plain click or shift+click: selection is applied unconditionally inside
    // navigateToSource (mirrors the Ctrl+click toggleSelect path). Editor
    // source-scroll is best-effort — silently skipped for realized/derived
    // geometry (boolean results, patterns, auto-generated sub-entities) that
    // has no 1:1 source span.
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
      <Show when={showPalette()}>
        <CommandPalette
          getCommands={paletteCommands}
          runCommand={(id) => runCommand(id, shortcutCallbacks)}
          fetchSymbols={fetchPaletteSymbols}
          filePath={editorStore.state.activeFile ?? ''}
          onJumpToLocation={(loc) => setScrollToLocation(loc)}
          onClose={() => setShowPalette(false)}
          initialMode={paletteMode()}
        />
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
            style={{ 'grid-template-columns': `${layoutStore.state.editorWidth}px 4px 1fr 4px ${layoutStore.state.sideWidth}px` }}
          >
            <div ref={editorPanelRef} data-testid="editor-panel" class={styles.editorPanel}>
              <FileBrowser
                files={editorStore.state.openFiles}
                activeFile={editorStore.state.activeFile}
                onFileClick={handleFileClick}
              />
              <FileTabs store={editorStore} />
              <Editor store={editorStore} scrollToLocation={scrollToLocation} onOpen={handleOpen} onError={(msg) => showToast(msg, 'error')} onSaveConflict={(file) => showSaveConflictPrompt(file)} compileDiagnostics={engineStore.state.compileDiagnostics} liveContentRef={(fn) => { getLiveEditorContent = fn; }} onShowReferences={(results) => { setFindUsesResults(results); setFindUsesOpen(true); }} />
              {/* Horizontal splitter — shown only when diagnostics panel is expanded */}
              <Show when={!layoutStore.state.problemsCollapsed}>
                <Splitter orientation="horizontal" data-testid="splitter-problems" onResize={handleProblemsResize} />
              </Show>
              {/* Docked diagnostics panel — always mounted, collapsed by default */}
              <DiagnosticsPanel
                collapsed={layoutStore.state.problemsCollapsed}
                height={layoutStore.state.problemsHeight}
                diagnostics={allDiagnostics()}
                onToggleCollapsed={handleToggleDiagnostics}
                onNavigate={handleNavigateToDiagnostic}
              />
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
                const base = `${layoutStore.state.designTreeHeight}px 4px ${layoutStore.state.propertyHeight}px 4px`;
                // Middle tracks: one `auto` per optional panel present (autoResolve then
                // mechanism). No splitter between cons and the first optional panel — adding
                // one would shift subsequent children up a track and collapse chat into 4px.
                const midTracks = [
                  ...(hasAR ? ['auto'] : []),
                  ...(hasMech ? ['auto'] : []),
                ];
                const midStr = midTracks.length > 0
                  ? `${layoutStore.state.constraintHeight}px ${midTracks.join(' ')}`
                  : null;
                if (hasChat) {
                  return `${base} ${midStr ?? `${layoutStore.state.constraintHeight}px`} 4px minmax(${CHAT_MIN_HEIGHT}px, 1fr)`;
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
                onHover={(path) => selectionStore.hoverEntity(path)}
                hoveredEntity={selectionStore.state.hoveredEntity}
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
              {/* BucklingPanel: shown when the buckling solver has emitted mode-shape data */}
              <Show when={(bucklingStore.state.base !== null) || bucklingStore.modes().length > 0}>
                <BucklingPanel store={bucklingStore} />
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
            diagnosticsCollapsed={layoutStore.state.problemsCollapsed}
          />
          <ExportDialog
            open={showExportDialog()}
            exporting={exporting()}
            onExport={handleDoExport}
            onClose={() => setShowExportDialog(false)}
          />
          {/* DiagnosticsPanel relocated to .editorPanel (docked) — see below */}
          <FindUsesPanel
            open={findUsesOpen()}
            results={findUsesResults()}
            onClose={() => setFindUsesOpen(false)}
            onNavigate={(r) => {
              // LSP positions are 0-based; SourceLocation/cursor is 1-based (+1).
              // Reuses the diagnostics setScrollToLocation path, which moves the
              // cursor AND records the ζ same-file nav-history entry (Editor.tsx
              // scrollToLocation effect) — so nav-history needs zero new plumbing.
              setScrollToLocation({
                file_path: r.uri,
                line: r.line + 1,
                column: r.character + 1,
                end_line: r.endLine + 1,
                end_column: r.endCharacter + 1,
              });
              setFindUsesOpen(false);
            }}
          />
          <ViewManageModal
            open={viewManageOpen()}
            store={viewStateStore}
            onClose={() => setViewManageOpen(false)}
          />
          {/* WarmPoolDebugPanel: REIFY_DEBUG=1 only — floating overlay, task 4279 */}
          <Show when={debugEnabled()}>
            <div class={styles.warmPoolDebugOverlay} data-testid="warm-pool-debug-overlay">
              <WarmPoolDebugPanel />
            </div>
          </Show>
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
