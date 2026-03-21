import { type Component, onMount, onCleanup, createSignal, createEffect, Show } from 'solid-js';
import { applyTheme } from './theme';
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
import { useKeyboardShortcuts } from './hooks/useKeyboardShortcuts';
import { createToast } from './hooks/useToast';
import { createEngineStore } from './stores/engineStore';
import { createEditorStore } from './stores/editorStore';
import { createSelectionStore } from './stores/selectionStore';
import {
  getInitialState,
  setParameter as bridgeSetParameter,
  exportGeometry as bridgeExportGeometry,
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
import type { ExportFormat, FileData, SourceLocation, ConstraintData } from './types';
import styles from './App.module.css';

const MIN_PANEL_WIDTH = 150;
const MIN_PANEL_HEIGHT = 80;
const DEFAULT_EDITOR_WIDTH = 300;
const DEFAULT_SIDE_WIDTH = 300;
const DEFAULT_PROPERTY_HEIGHT = 200;

const App: Component = () => {
  const editorStore = createEditorStore();
  const selectionStore = createSelectionStore();
  const engineStore = createEngineStore({
    onEntityRemoved: (id) => selectionStore.clearIfRemoved(id),
  });

  const [editorWidth, setEditorWidth] = createSignal(DEFAULT_EDITOR_WIDTH);
  const [sideWidth, setSideWidth] = createSignal(DEFAULT_SIDE_WIDTH);
  const [propertyHeight, setPropertyHeight] = createSignal(DEFAULT_PROPERTY_HEIGHT);

  // Init phase: loading → ready | error
  const [initPhase, setInitPhase] = createSignal<'loading' | 'ready' | 'error'>('loading');

  // Export dialog state
  const [showExportDialog, setShowExportDialog] = createSignal(false);
  const [exporting, setExporting] = createSignal(false);

  // Toast state (centralized via createToast hook)
  const toast = createToast();

  // Reload prompt state
  const [changedFile, setChangedFile] = createSignal<string | null>(null);

  // Navigation state
  const [scrollToLocation, setScrollToLocation] = createSignal<SourceLocation | null>(null);
  let flyToEntityFn: ((entityPath: string) => void) | undefined;

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
    onOpen: () => {
      // Open file via bridge (placeholder — would use native dialog in full app)
      // For now, this is a stub that can be wired to a native file picker
    },
    onReEvaluate: () => {
      // Re-evaluate the active file
      const activeFile = editorStore.state.activeFile;
      if (activeFile) {
        const file = editorStore.state.openFiles.find((f) => f.path === activeFile);
        if (file) {
          bridgeUpdateSource(file.path, file.content).catch((err) =>
            console.error('Re-evaluate failed:', err),
          );
        }
      }
    },
    onExportDialog: () => {
      setShowExportDialog((v) => !v);
    },
  });

  let alive = true;
  let unsub: (() => void) | undefined;
  let fileChangedUnsub: (() => void) | undefined;

  async function initApp() {
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
    setInitPhase('ready');

    try {
      const u = await engineStore.subscribeToEvents();
      if (!alive) {
        u();
        return;
      }
      unsub = u;
    } catch (err) {
      console.error('Failed to subscribe to events:', err);
    }

    // Subscribe to file-changed events
    try {
      const unlistenFileChanged = await onFileChanged((data: FileData) => {
        // Only show reload prompt if the file is currently open
        const isOpen = editorStore.state.openFiles.some((f) => f.path === data.path);
        if (isOpen) {
          setChangedFile(data.path);
        }
      });
      if (!alive) {
        unlistenFileChanged();
        return;
      }
      fileChangedUnsub = unlistenFileChanged;
    } catch (err) {
      console.error('Failed to subscribe to file changes:', err);
    }
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
      toast.showToast(`Parameter update failed: ${err instanceof Error ? err.message : String(err)}`, 'error'),
    );
  }

  function handleExport() {
    setShowExportDialog(true);
  }

  async function handleDoExport(format: ExportFormat) {
    setExporting(true);
    try {
      // In a full app, would open native save dialog here
      const defaultPath = `export.${format}`;
      await bridgeExportGeometry(format, defaultPath);
      toast.showToast(`Exported successfully as ${format.toUpperCase()}`, 'success');
    } catch (err) {
      toast.showToast(`Export failed: ${err instanceof Error ? err.message : String(err)}`, 'error');
    } finally {
      setExporting(false);
      setShowExportDialog(false);
    }
  }

  function handleFitToView() {
    // Fit-to-view stub — will be wired to viewport camera reset in a future task
  }

  function handleReload() {
    const path = changedFile();
    if (path) {
      bridgeOpenFile(path)
        .then((fileData) => {
          editorStore.updateFileContent(fileData.path, fileData.content);
          setChangedFile(null);
        })
        .catch((err) => console.error('Reload failed:', err));
    }
  }

  function handleDismissReload() {
    setChangedFile(null);
  }

  function handleRetry() {
    initApp();
  }

  function handleDismissToast() {
    toast.dismissToast();
  }

  function handleFileClick(path: string) {
    editorStore.setActiveFile(path);
  }

  function handleLeftResize(delta: number) {
    setEditorWidth((w) => Math.max(MIN_PANEL_WIDTH, w + delta));
  }

  function handleRightResize(delta: number) {
    setSideWidth((w) => Math.max(MIN_PANEL_WIDTH, w - delta));
  }

  function handleSideResize(delta: number) {
    setPropertyHeight((h) => Math.max(MIN_PANEL_HEIGHT, h + delta));
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
      <Show when={initPhase() === 'loading'}>
        <div data-testid="app-loading" class={styles.loading}>
          <div class={styles.spinner} />
          <p>Loading...</p>
        </div>
      </Show>
      <Show when={initPhase() === 'error'}>
        <div data-testid="app-error" class={styles.errorState}>
          <p>Failed to load application state.</p>
          <button onClick={handleRetry}>Retry</button>
        </div>
      </Show>
      <Show when={initPhase() === 'ready'}>
        <div data-testid="app-layout" class={styles.layout}>
          <Toolbar onExport={handleExport} onFitToView={handleFitToView} />
          <ReloadPrompt
            filePath={changedFile()}
            onReload={handleReload}
            onDismiss={handleDismissReload}
          />
          <div
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
              <Editor store={editorStore} scrollToLocation={scrollToLocation} />
            </div>
            <Splitter orientation="vertical" onResize={handleLeftResize} data-testid="splitter-left" />
            <div data-testid="viewport-panel" class={styles.viewportPanel}>
              <Viewport
                meshes={engineStore.state.meshes}
                onSelect={handleViewportSelect}
                selectedEntity={selectionStore.state.selectedEntity}
                hoveredEntity={selectionStore.state.hoveredEntity}
                flyToEntityRef={(fn) => { flyToEntityFn = fn; }}
              />
            </div>
            <Splitter orientation="vertical" onResize={handleRightResize} data-testid="splitter-right" />
            <div
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
          <Show when={toast.toastMessage()}>
            {(msg) => (
              <div class={styles.toastContainer}>
                <Toast message={msg()} type={toast.toastType()} onDismiss={handleDismissToast} />
              </div>
            )}
          </Show>
        </div>
      </Show>
    </>
  );
};

export default App;
