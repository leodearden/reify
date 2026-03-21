import { type Component, onMount, onCleanup, createSignal, createEffect, For } from 'solid-js';
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
import type { ExportFormat, FileData, SourceLocation, ConstraintData, ToastMessage } from './types';
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

  const [editorWidth, setEditorWidth] = createSignal(DEFAULT_EDITOR_WIDTH);
  const [sideWidth, setSideWidth] = createSignal(DEFAULT_SIDE_WIDTH);
  const [propertyHeight, setPropertyHeight] = createSignal(DEFAULT_PROPERTY_HEIGHT);

  // Export dialog state
  const [showExportDialog, setShowExportDialog] = createSignal(false);
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

  onMount(async () => {
    applyTheme();

    try {
      const initialState = await getInitialState();
      if (!alive) return;
      engineStore.initFromState(initialState);
      for (const file of initialState.files) {
        editorStore.openFile(file);
      }
    } catch (err) {
      console.error('Failed to load initial state:', err);
    }

    if (!alive) return;

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
          setChangedFiles((prev) => new Set([...prev, data.path]));
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
  });

  onCleanup(() => {
    alive = false;
    unsub?.();
    fileChangedUnsub?.();
  });

  function handleSetParameter(cellId: string, value: string) {
    bridgeSetParameter(cellId, value).catch((err) =>
      console.error('setParameter failed:', err),
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
      showToast(`Exported successfully as ${format.toUpperCase()}`, 'success');
    } catch (err) {
      showToast(`Export failed: ${err instanceof Error ? err.message : String(err)}`, 'error');
    } finally {
      setExporting(false);
      setShowExportDialog(false);
    }
  }

  function handleFitToView() {
    // Fit-to-view stub — will be wired to viewport camera reset in a future task
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

    const promises = Array.from(files).map((path) =>
      bridgeOpenFile(path)
        .then((fileData) => {
          editorStore.updateFileContent(fileData.path, fileData.content);
        }),
    );
    Promise.all(promises)
      .then(() => {
        setChangedFiles(new Set());
        setConfirmReload(false);
      })
      .catch((err) => console.error('Reload failed:', err));
  }

  function handleDismissReload() {
    setChangedFiles(new Set());
    setConfirmReload(false);
  }

  function handleDismissToast(id: string) {
    setToasts((prev) => prev.filter((t) => t.id !== id));
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
    <div data-testid="app-layout" class={styles.layout}>
      <Toolbar onExport={handleExport} onFitToView={handleFitToView} />
      <ReloadPrompt
        filePaths={Array.from(changedFiles())}
        hasDirtyFiles={confirmReload()}
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
      <div class={styles.toastContainer}>
        <For each={toasts()}>
          {(toast) => (
            <Toast
              message={toast.message}
              type={toast.type}
              onDismiss={() => handleDismissToast(toast.id)}
            />
          )}
        </For>
      </div>
    </div>
  );
};

export default App;
