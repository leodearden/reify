import { type Component, onMount } from 'solid-js';
import { applyTheme } from './theme';
import { Viewport } from './viewport';
import { Editor } from './editor/Editor';
import { FileTabs } from './editor/FileTabs';
import { PropertyEditor, ConstraintPanel, Toolbar, StatusBar } from './panels';
import { createEngineStore } from './stores/engineStore';
import { createEditorStore } from './stores/editorStore';
import { createSelectionStore } from './stores/selectionStore';
import { setParameter as bridgeSetParameter } from './bridge';
import styles from './App.module.css';

const App: Component = () => {
  const engineStore = createEngineStore();
  const editorStore = createEditorStore();
  const selectionStore = createSelectionStore();

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

  return (
    <div data-testid="app-layout" class={styles.layout}>
      <Toolbar onExport={handleExport} onFitToView={handleFitToView} />
      <div class={styles.main}>
        <div data-testid="editor-panel" class={styles.editorPanel}>
          <FileTabs store={editorStore} />
          <Editor store={editorStore} />
        </div>
        <div data-testid="viewport-panel" class={styles.viewportPanel}>
          <Viewport meshes={engineStore.state.meshes} />
        </div>
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
