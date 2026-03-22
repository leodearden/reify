import { type Component, onMount, onCleanup, createSignal, createEffect, Show, For } from 'solid-js';
import { Viewport } from './viewport';
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
} from './panels';
import { Splitter } from './components/Splitter';
import { KeyboardHelp } from './components/KeyboardHelp';
import { useKeyboardShortcuts } from './hooks/useKeyboardShortcuts';
import { createEngineStore } from './stores/engineStore';
import { createEditorStore } from './stores/editorStore';
import { createSelectionStore } from './stores/selectionStore';
import {
  getInitialState,
  setParameter as bridgeSetParameter,
  exportGeometry as bridgeExportGeometry,
  pickSavePath,
  pickOpenPath,
  updateSource as bridgeUpdateSource,
  openFile as bridgeOpenFile,
  onFileChanged,
  getSourceLocation as bridgeGetSourceLocation,
  focusEntity as bridgeFocusEntity,
} from './bridge';
import {
  navigateToSource,
  navigateToEntity,
  navigateFromConstraint,
} from './navigation';
import type { ExportFormat, FileData, SourceLocation, ConstraintData, ToastMessage } from './types';
import { applyTheme } from './theme';
import { loadPanelLayout, savePanelLayout } from './hooks/useLayoutPersistence';
import styles from './App.module.css';

const MIN_PANEL_WIDTH = 150;
const MIN_PANEL_HEIGHT = 80;
const DEFAULT_EDITOR_WIDTH = 300;
const DEFAULT_SIDE_WIDTH = 300;
const DEFAULT_PROPERTY_HEIGHT = 200;

let toastIdCounter = 0;

const App: Component = () => {
  const editorStore = createEditorStore();
  const selectionStore = createSelectionStore();
  const engineStore = createEngineStore({
    onEntityRemoved: (id) => selectionStore.clearIfRemoved(id),
  });

  const savedLayout = loadPanelLayout();
  const [editorWidth, setEditorWidth] = createSignal(savedLayout?.editorWidth ?? DEFAULT_EDITOR_WIDTH);
  const [sideWidth, setSideWidth] = createSignal(savedLayout?.sideWidth ?? DEFAULT_SIDE_WIDTH);
  const [propertyHeight, setPropertyHeight] = createSignal(savedLayout?.propertyHeight ?? DEFAULT_PROPERTY_HEIGHT);

  // Debounced persistence of panel layout dimensions
  let saveTimeout: ReturnType<typeof setTimeout> | undefined;
  createEffect(() => {
    const layout = {
      editorWidth: editorWidth(),
      sideWidth: sideWidth(),
      propertyHeight: propertyHeight(),
    };
    clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => savePanelLayout(layout), 300);
  });

  // Init phase: loading → ready | error
  const [initPhase, setInitPhase] = createSignal<'loading' | 'ready' | 'error'>('loading');

  // Export dialog state
  const [showExportDialog, setShowExportDialog] = createSignal(false);

  // Keyboard help overlay state
  const [showHelp, setShowHelp] = createSignal(false);
  const [exporting, setExporting] = createSignal(false);

  // Toast queue state
  const [toasts, setToasts] = createSignal<ToastMessage[]>([]);

  function showToast(message: string, type: ToastMessage['type']) {
    const id = String(++toastIdCounter);
    setToasts((prev) => [...prev, { id, type, message }]);
  }

  // Reload prompt state — tracks all files changed since last reload/dismiss
  const [changedFiles, setChangedFiles] = createSignal<Set<string>>(new Set());
  const [confirmReload, setConfirmReload] = createSignal(false);

  // Navigation state
  const [scrollToLocation, setScrollToLocation] = createSignal<SourceLocation | null>(null);
  let flyToEntityFn: ((entityPath: string) => void) | undefined;
  let fitToViewFn: (() => void) | undefined;

  // Refs for splitter max-width clamping
  let mainRef: HTMLDivElement | undefined;
  let sidePanelRef: HTMLDivElement | undefined;

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

  // Keyboard shortcuts
  useKeyboardShortcuts({
    onOpen: async () => {
      try {
        const path = await pickOpenPath();
        if (!path) return;
        const fileData = await bridgeOpenFile(path);
        editorStore.openFile(fileData);
      } catch (err) {
        showToast(`Open file failed: ${err instanceof Error ? err.message : String(err)}`, 'error');
      }
    },
    onReEvaluate: () => {
      // Re-evaluate the active file
      const activeFile = editorStore.state.activeFile;
      if (activeFile) {
        const file = editorStore.state.openFiles.find((f) => f.path === activeFile);
        if (file) {
          bridgeUpdateSource(file.path, file.content).catch((err) =>
            showToast(`Re-evaluation failed: ${err instanceof Error ? err.message : String(err)}`, 'error'),
          );
        }
      }
    },
    onExportDialog: () => {
      setShowExportDialog((v) => !v);
    },
    onHelp: () => {
      setShowHelp((v) => !v);
    },
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
  });

  let alive = true;
  let unsub: (() => void) | undefined;
  let fileChangedUnsub: (() => void) | undefined;

  async function initApp() {
    // Clean up existing subscriptions before proceeding (defensive against
    // concurrent or re-entrant initApp calls, e.g. rapid retry)
    unsub?.();
    unsub = undefined;
    fileChangedUnsub?.();
    fileChangedUnsub = undefined;

    setInitPhase('loading');

    try {
      const initialState = await getInitialState();
      if (!alive) return;
      engineStore.initFromState(initialState);
      for (const file of initialState.files) {
        editorStore.openFile(file);
      }
    } catch (_err) {
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
        // Only show reload prompt if the file is currently open
        const isOpen = editorStore.state.openFiles.some((f) => f.path === data.path);
        if (isOpen) {
          setChangedFiles((prev) => new Set([...prev, data.path]));
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

    if (!alive) return;
    setInitPhase('ready');
  }

  onMount(() => {
    applyTheme();
    initApp();
  });

  onCleanup(() => {
    alive = false;
    unsub?.();
    fileChangedUnsub?.();
  });

  function handleSetParameter(cellId: string, value: string) {
    bridgeSetParameter(cellId, value).catch((err) =>
      showToast(`Parameter update failed: ${err instanceof Error ? err.message : String(err)}`, 'error'),
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
      showToast(`Could not open save dialog: ${err instanceof Error ? err.message : String(err)}`, 'error');
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
      showToast(`Export failed: ${err instanceof Error ? err.message : String(err)}`, 'error');
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
    setChangedFiles(new Set());
    setConfirmReload(false);
  }

  function handleRetry() {
    initApp();
  }

  function handleDismissToast(id: string) {
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

  function handleSideResize(delta: number) {
    const ch = sidePanelRef?.clientHeight ?? 0;
    const maxHeight = ch > 0 ? ch - MIN_PANEL_HEIGHT - 4 : Infinity;
    setPropertyHeight((h) => Math.min(maxHeight, Math.max(MIN_PANEL_HEIGHT, h + delta)));
  }

  function handleViewportSelect(entityPath: string | null) {
    if (!entityPath) {
      selectionStore.selectEntity(null);
      return;
    }
    navigateToSource(entityPath, {
      getSourceLocation: bridgeGetSourceLocation,
      scrollEditor: (loc) => setScrollToLocation(loc),
      selectEntity: (ep) => selectionStore.selectEntity(ep),
    });
  }

  function handleGroupDoubleClick(groupName: string) {
    navigateToEntity(groupName, {
      focusEntity: bridgeFocusEntity,
      flyToEntity: (ep) => flyToEntityFn?.(ep),
      selectEntity: (ep) => selectionStore.selectEntity(ep),
    });
  }

  function handleConstraintSelect(constraint: ConstraintData) {
    const valuesArray = Object.values(engineStore.state.values);
    navigateFromConstraint(constraint, valuesArray, {
      selectEntity: (ep) => selectionStore.selectEntity(ep),
      setHighlightedParams: (ids) => selectionStore.setHighlightedParams(ids),
    });
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
              <Editor store={editorStore} scrollToLocation={scrollToLocation} onError={(msg) => showToast(msg, 'error')} />
            </div>
            <Splitter orientation="vertical" onResize={handleLeftResize} data-testid="splitter-left" />
            <div data-testid="viewport-panel" class={styles.viewportPanel}>
              <Viewport
                meshes={engineStore.state.meshes}
                onSelect={handleViewportSelect}
                onHover={(path) => selectionStore.hoverEntity(path)}
                selectedEntity={selectionStore.state.selectedEntity}
                hoveredEntity={selectionStore.state.hoveredEntity}
                evalStatus={engineStore.state.evalStatus}
                flyToEntityRef={(fn) => { flyToEntityFn = fn; }}
                fitToViewRef={(fn) => { fitToViewFn = fn; }}
              />
            </div>
            <Splitter orientation="vertical" onResize={handleRightResize} data-testid="splitter-right" />
            <div
              ref={sidePanelRef}
              data-testid="side-panel"
              class={styles.sidePanel}
              style={{ 'grid-template-rows': `${propertyHeight()}px 4px 1fr` }}
            >
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
              />
            </div>
          </div>
          <StatusBar
            evalStatus={engineStore.state.evalStatus}
            meshes={engineStore.state.meshes}
            constraints={engineStore.state.constraints}
          />
          <ExportDialog
            open={showExportDialog()}
            exporting={exporting()}
            onExport={handleDoExport}
            onClose={() => setShowExportDialog(false)}
          />
          <div class={styles.toastContainer}>
            <For each={toasts()}>
              {(t) => (
                <Toast
                  message={t.message}
                  type={t.type}
                  onDismiss={() => handleDismissToast(t.id)}
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
