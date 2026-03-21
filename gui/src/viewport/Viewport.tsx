import { onMount, onCleanup, createEffect, Show } from 'solid-js';
import type { MeshData, EvaluationStatus } from '../types';
import { createScene } from './scene';
import { createControls } from './controls';
import { createMeshManager } from './meshManager';
import { createSelection } from './selection';

export interface ViewportProps {
  meshes: Record<string, MeshData>;
  onHover?: (path: string | null) => void;
  onSelect?: (path: string | null) => void;
  hoveredEntity?: string | null;
  selectedEntity?: string | null;
  evalStatus?: EvaluationStatus;
  onFitToView?: () => void;
}

export function Viewport(props: ViewportProps) {
  let canvasRef!: HTMLCanvasElement;
  let containerRef!: HTMLDivElement;
  let doFitToView: (() => void) | undefined;

  onMount(() => {
    const rect = containerRef.getBoundingClientRect();
    const width = rect.width || 800;
    const height = rect.height || 600;

    const { scene, camera, renderer, resize } = createScene(canvasRef, width, height);
    const controls = createControls(camera, renderer.domElement);
    const meshManager = createMeshManager(scene);

    // Create selection system
    const selection = createSelection({
      scene,
      camera,
      domElement: renderer.domElement,
      getMeshes: () => meshManager.getSceneMeshes(),
      onHover: (path) => props.onHover?.(path),
      onSelect: (path) => props.onSelect?.(path),
    });

    doFitToView = () => selection.fitToView();

    // Sync meshes reactively
    createEffect(() => {
      meshManager.sync(props.meshes);
    });

    // Sync hover/selection state from props to selection module
    createEffect(() => {
      selection.setHovered(props.hoveredEntity ?? null);
    });

    createEffect(() => {
      void props.meshes;
      selection.setSelected(props.selectedEntity ?? null);
    });

    // Resize handling via ResizeObserver
    const resizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width: w, height: h } = entry.contentRect;
        if (w > 0 && h > 0) {
          resize(w, h);
          selection.invalidateRect();
        }
      }
    });
    resizeObserver.observe(containerRef);

    // Animation loop with disposed guard to prevent race condition
    let disposed = false;
    let animationFrameId = 0;
    function animate() {
      if (disposed) return;
      animationFrameId = requestAnimationFrame(animate);
      controls.update();
      renderer.render(scene, camera);
    }
    animate();

    // Cleanup — set disposed first to guard any in-flight RAF callback
    onCleanup(() => {
      disposed = true;
      cancelAnimationFrame(animationFrameId);
      resizeObserver.disconnect();
      selection.dispose();
      controls.dispose();
      meshManager.dispose();
      renderer.dispose();
    });
  });

  return (
    <div
      ref={containerRef}
      data-testid="viewport-container"
      style={{ width: '100%', height: '100%', position: 'relative' }}
    >
      <canvas ref={canvasRef} data-testid="viewport-canvas" />

      {/* Tooltip overlay */}
      <Show when={props.hoveredEntity}>
        <div
          data-testid="viewport-tooltip"
          style={{
            position: 'absolute',
            top: '8px',
            left: '8px',
            padding: '4px 8px',
            'background-color': 'var(--reify-surface, #2a2a3a)',
            color: 'var(--reify-text, #cdd6f4)',
            'border-radius': '4px',
            'font-size': '12px',
            'pointer-events': 'none',
            'z-index': '10',
          }}
        >
          {props.hoveredEntity}
        </div>
      </Show>

      {/* Evaluating spinner overlay */}
      <Show when={props.evalStatus && props.evalStatus.phase !== 'idle'}>
        <div
          data-testid="viewport-spinner"
          style={{
            position: 'absolute',
            top: '50%',
            left: '50%',
            transform: 'translate(-50%, -50%)',
            padding: '12px 20px',
            'background-color': 'rgba(30, 30, 46, 0.85)',
            color: 'var(--reify-accent, #89b4fa)',
            'border-radius': '8px',
            'font-size': '14px',
            'z-index': '10',
          }}
        >
          Evaluating...
        </div>
      </Show>

      {/* Fit-to-view button */}
      <button
        data-testid="fit-to-view"
        onClick={() => { doFitToView?.(); props.onFitToView?.(); }}
        style={{
          position: 'absolute',
          bottom: '12px',
          right: '12px',
          padding: '6px 10px',
          'background-color': 'var(--reify-surface, #2a2a3a)',
          color: 'var(--reify-text, #cdd6f4)',
          border: '1px solid var(--reify-border, #45475a)',
          'border-radius': '4px',
          cursor: 'pointer',
          'font-size': '12px',
          'z-index': '10',
        }}
        title="Fit to view"
      >
        Fit
      </button>
    </div>
  );
}
