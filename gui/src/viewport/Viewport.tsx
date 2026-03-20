import { onMount, onCleanup, createEffect } from 'solid-js';
import type { MeshData } from '../types';
import { createScene } from './scene';
import { createControls } from './controls';
import { createMeshManager } from './meshManager';

export interface ViewportProps {
  meshes: Record<string, MeshData>;
}

export function Viewport(props: ViewportProps) {
  let canvasRef!: HTMLCanvasElement;
  let containerRef!: HTMLDivElement;

  onMount(() => {
    const rect = containerRef.getBoundingClientRect();
    const width = rect.width || 800;
    const height = rect.height || 600;

    const { scene, camera, renderer, resize } = createScene(canvasRef, width, height);
    const controls = createControls(camera, renderer.domElement);
    const meshManager = createMeshManager(scene);

    // Sync meshes reactively
    createEffect(() => {
      meshManager.sync(props.meshes);
    });

    // Resize handling via ResizeObserver
    const resizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width: w, height: h } = entry.contentRect;
        if (w > 0 && h > 0) {
          resize(w, h);
        }
      }
    });
    resizeObserver.observe(containerRef);

    // Animation loop
    let animationFrameId: number;
    function animate() {
      animationFrameId = requestAnimationFrame(animate);
      controls.update();
      renderer.render(scene, camera);
    }
    animate();

    // Cleanup
    onCleanup(() => {
      cancelAnimationFrame(animationFrameId);
      resizeObserver.disconnect();
      controls.dispose();
      meshManager.dispose();
      renderer.dispose();
    });
  });

  return (
    <div
      ref={containerRef}
      data-testid="viewport-container"
      style={{ width: '100%', height: '100%' }}
    >
      <canvas ref={canvasRef} data-testid="viewport-canvas" />
    </div>
  );
}
