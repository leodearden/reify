import { type Component, onMount, createSignal } from 'solid-js';
import { applyTheme } from './theme';
import { Viewport } from './viewport';
import { Editor } from './editor/Editor';
import { FileTabs } from './editor/FileTabs';
import { PropertyEditor, ConstraintPanel, Toolbar, StatusBar } from './panels';
import { Splitter } from './components/Splitter';
import { createEngineStore } from './stores/engineStore';
import { createEditorStore } from './stores/editorStore';
import { createSelectionStore } from './stores/selectionStore';
import { setParameter as bridgeSetParameter } from './bridge';
import styles from './App.module.css';

const MIN_PANEL_WIDTH = 150;
const DEFAULT_EDITOR_WIDTH = 300;
const DEFAULT_SIDE_WIDTH = 300;

const App: Component = () => {
  const engineStore = createEngineStore();
  const editorStore = createEditorStore();
  const selectionStore = createSelectionStore();

  const [editorWidth, setEditorWidth] = createSignal(DEFAULT_EDITOR_WIDTH);
  const [sideWidth, setSideWidth] = createSignal(DEFAULT_SIDE_WIDTH);

  onMount(() => {
    applyTheme();
  });

  function handleSetParameter(cellId: string, value: string) {
    bridgeSetParameter(cellId, value);
  }

  function handleExport() {
    // Export stub — will be wired to export dialog in a future task
  }

  function handleFitToView() {
    // Fit-to-view stub — will be wired to viewport camera reset in a future task
  }

  function handleLeftResize(delta: number) {
    setEditorWidth((w) => Math.max(MIN_PANEL_WIDTH, w + delta));
  }

  function handleRightResize(delta: number) {
    setSideWidth((w) => Math.max(MIN_PANEL_WIDTH, w - delta));
  }

  return (
    <div data-testid="app-layout" class={styles.layout}>
      <Toolbar onExport={handleExport} onFitToView={handleFitToView} />
      <div
        class={styles.main}
        style={{ 'grid-template-columns': `${editorWidth()}px 4px 1fr 4px ${sideWidth()}px` }}
      >
        <div data-testid="editor-panel" class={styles.editorPanel}>
          <FileTabs store={editorStore} />
          <Editor store={editorStore} />
        </div>
        <Splitter orientation="vertical" onResize={handleLeftResize} data-testid="splitter-left" />
        <div data-testid="viewport-panel" class={styles.viewportPanel}>
          <Viewport meshes={engineStore.state.meshes} />
        </div>
        <Splitter orientation="vertical" onResize={handleRightResize} data-testid="splitter-right" />
        <div data-testid="side-panel" class={styles.sidePanel}>
          <PropertyEditor
            values={engineStore.state.values}
            selectedEntity={selectionStore.state.selectedEntity}
            onSetParameter={handleSetParameter}
          />
          <ConstraintPanel
            constraints={engineStore.state.constraints}
            values={engineStore.state.values}
          />
        </div>
      </div>
      <StatusBar
        evalStatus={engineStore.state.evalStatus}
        meshes={engineStore.state.meshes}
        constraints={engineStore.state.constraints}
      />
    </div>
  );
};

export default App;
