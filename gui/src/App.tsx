import { type Component, onMount } from 'solid-js';
import { applyTheme } from './theme';
import styles from './App.module.css';

const App: Component = () => {
  onMount(() => {
    applyTheme();
  });

  return (
    <div data-testid="app-layout" class={styles.layout}>
      <div data-testid="editor-panel" class={styles.panel}>
        <h3>Editor</h3>
        <div class={styles.placeholder}>Code editor placeholder</div>
      </div>
      <div data-testid="viewport-panel" class={styles.panel}>
        <h3>3D Viewport</h3>
        <div class={styles.placeholder}>Viewport placeholder</div>
      </div>
      <div data-testid="side-panel" class={styles.sidePanel}>
        <div data-testid="property-editor-panel" class={styles.panel}>
          <h3>Properties</h3>
          <div class={styles.placeholder}>Property editor placeholder</div>
        </div>
        <div data-testid="constraints-panel" class={styles.panel}>
          <h3>Constraints</h3>
          <div class={styles.placeholder}>Constraints panel placeholder</div>
        </div>
      </div>
    </div>
  );
};

export default App;
