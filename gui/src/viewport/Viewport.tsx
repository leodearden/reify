import { onMount, onCleanup, createEffect, createSignal, Show } from 'solid-js';
import type { MeshData, EvaluationStatus, VisibilityState } from '../types';
import { Box3 } from 'three';
import { createScene } from './scene';
import { createControls } from './controls';
import { createMeshManager } from './meshManager';
import { createSelection } from './selection';
import { FeaModeToolbar } from './FeaModeToolbar';
import { bakeColours } from './colormap';
import type { ViewportStore, CameraState, FeaModeStore } from '../stores';

export interface ViewportProps {
  /**
   * Stable identifier for this viewport instance (e.g. "design-main"). Required.
   *
   * **Captured at mount** — this value (and `viewportStore` below) are read once
   * inside `onMount` and must not change for the lifetime of the component. If the
   * parent needs to repurpose the canvas for a different viewport, unmount and
   * remount the `<Viewport>` with the new `viewportId`.
   */
  viewportId: string;
  meshes: Record<string, MeshData>;
  onHover?: (path: string | null) => void;
  onSelect?: (path: string | null, modifiers?: { ctrl: boolean; shift: boolean }) => void;
  hoveredEntity?: string | null;
  selectedEntity?: string | null;
  /** Multi-selection list. When provided, takes precedence over selectedEntity for wireframe rendering. */
  selectedEntities?: readonly string[];
  evalStatus?: EvaluationStatus;
  onFitToView?: () => void;
  flyToEntityRef?: (fn: (entityPath: string) => void) => void;
  fitToViewRef?: (fn: () => void) => void;
  entityVisibility?: Record<string, VisibilityState>;
  /**
   * Optional viewport store. When provided, the saved camera state for
   * `viewportId` is applied on mount and persisted once per interaction via
   * the OrbitControls `'end'` event (fires once when the user releases
   * pointer/touch — not on every damping frame). When absent, camera state
   * is ephemeral (existing behaviour).
   *
   * **Captured at mount** — like `viewportId`, this reference is captured
   * once inside `onMount` and must not change for the component lifetime.
   * To swap the store, unmount and remount the `<Viewport>`.
   */
  viewportStore?: ViewportStore;
  /**
   * Optional FEA-mode store. When provided, renders a `<FeaModeToolbar>`
   * overlay and bridges store state changes into the meshManager colorize
   * pipeline. When absent, no FEA UI is rendered (existing behaviour).
   *
   * **Captured at mount** — captured once inside `onMount`. Swap by
   * unmounting and remounting the `<Viewport>`.
   */
  feaModeStore?: FeaModeStore;
}

export function Viewport(props: ViewportProps) {
  let canvasRef!: HTMLCanvasElement;
  let containerRef!: HTMLDivElement;
  let doFitToView: (() => void) | undefined;
  const [showGrid, setShowGrid] = createSignal(true);
  const [pointerPos, setPointerPos] = createSignal({ x: 8, y: 8 });

  onMount(() => {
    const rect = containerRef.getBoundingClientRect();
    const width = rect.width || 800;
    const height = rect.height || 600;

    const { scene, camera, renderer, resize, adjustClipping, grid, axes } = createScene(canvasRef, width, height);
    const controls = createControls(camera, renderer.domElement);
    const meshManager = createMeshManager(scene);

    // Create selection system
    const selection = createSelection({
      scene,
      camera,
      domElement: renderer.domElement,
      getMeshes: () => meshManager.getSceneMeshes(),
      onHover: (path) => props.onHover?.(path),
      onSelect: (path, modifiers) => props.onSelect?.(path, modifiers),
      controls: controls.controls,
    });

    doFitToView = () => selection.fitToView();
    props.flyToEntityRef?.((entityPath: string) => selection.flyToEntity(entityPath));
    props.fitToViewRef?.(() => selection.fitToView());

    // Expose viewport internals for the debug bridge (REIFY_DEBUG=1)
    if (window.__REIFY_DEBUG__) {
      const debugEntry = {
        scene,
        camera,
        renderer,
        getMeshes: () => meshManager.getSceneMeshes(),
        getGhostMeshes: () => meshManager.getGhostMeshes(),
        fitToView: () => selection.fitToView(),
        flyToEntity: (entityPath: string) => selection.flyToEntity(entityPath),
        controls: controls.controls,
      };
      // Register in the map so sibling viewports don't clobber each other.
      window.__REIFY_DEBUG__.viewports ??= {};
      window.__REIFY_DEBUG__.viewports[props.viewportId] = debugEntry;
      // Keep the legacy single slot so direct-stub-injection tests still work.
      window.__REIFY_DEBUG__.viewport = debugEntry;
    }

    // Render-on-demand: keep rAF loop alive (for OrbitControls damping)
    // but only call renderer.render when something has changed.
    let needsRender = true;
    function requestRender() {
      needsRender = true;
    }

    // Restore saved camera from viewportStore before the first frame (if provided).
    // This must happen after requestRender is in scope but before reactive effects
    // and the animation loop start.
    const savedVp = props.viewportStore?.getViewport(props.viewportId);
    if (savedVp) {
      const cam = savedVp.camera;
      camera.position.set(...cam.position);
      camera.up.set(...cam.up);
      controls.controls.target.set(...cam.target);
      camera.zoom = cam.zoom ?? 1;
      camera.updateProjectionMatrix();
      controls.update();
      requestRender();
    }

    // Snapshot helper — reads current camera/controls state into a plain CameraState.
    // Guard .zoom with ?? 1 to tolerate mocks that may omit it.
    function snapshotCamera(): CameraState {
      return {
        position: [camera.position.x, camera.position.y, camera.position.z],
        target: [controls.controls.target.x, controls.controls.target.y, controls.controls.target.z],
        up: [camera.up.x, camera.up.y, camera.up.z],
        zoom: camera.zoom ?? 1,
      };
    }

    // Camera persistence handler — fires once when interaction ends ('end' event).
    // Using 'end' rather than 'change' avoids ~60 store writes/second during
    // OrbitControls damping; saves the last resting pose instead.
    // No-op when viewportStore is absent (ephemeral camera mode).
    function persistCamera() {
      props.viewportStore?.updateCamera(props.viewportId, snapshotCamera());
    }

    // OrbitControls 'change' event fires during camera movement (including damping)
    controls.controls.addEventListener('change', requestRender);
    // 'end' fires once per interaction (pointerup/touchend) — correct granularity for persistence
    controls.controls.addEventListener('end', persistCamera);

    // Bridge FEA-mode store → meshManager colorize pipeline.
    // Captured once at mount; the store reference must not change for the component lifetime.
    // Track-then-act pattern: reads all reactive dependencies (enabled/channel/palette/range)
    // at the top of the effect so any change to any of them rebuilds the bake closure.
    if (props.feaModeStore) {
      const feaStore = props.feaModeStore;
      // Performance note: channel/palette/range are read INSIDE the if(enabled) branch so
      // they are only tracked as reactive dependencies while FEA mode is active. When
      // enabled===false, only `enabled` is tracked, so changes to channel/palette/range do
      // NOT re-run the effect (and therefore do NOT redundantly call setColorize(null) +
      // rebuildMaterials() on every pre-configuration tweak).
      createEffect(() => {
        const enabled = feaStore.state.enabled;
        if (enabled) {
          const channel = feaStore.state.channel;
          const palette = feaStore.state.palette;
          const range = feaStore.state.range;
          meshManager.setColorize({
            channel,
            bake: (scalars: Float32Array) => bakeColours(scalars, range, palette),
          });
        } else {
          meshManager.setColorize(null);
          meshManager.rebuildMaterials();
        }
        requestRender();
      });

      // Bridge FEA deformed-shape view → meshManager geometry blend.
      // Track-then-act: `warpFactor` is read ONLY inside the `if (showDeformed)` branch so that
      // warpFactor changes do NOT re-run the effect (or call setDeformation redundantly) while
      // the deformed view is off. Mirrors the channel/palette/range gating in the colorize effect.
      createEffect(() => {
        const showDeformed = feaStore.state.showDeformed;
        if (showDeformed) {
          const warpFactor = feaStore.state.warpFactor;
          meshManager.setDeformation({ warpFactor });
        } else {
          meshManager.setDeformation(null);
        }
        requestRender();
      });
    }

    // Sync grid/axes visibility
    createEffect(() => {
      const visible = showGrid();
      grid.visible = visible;
      axes.visible = visible;
      requestRender();
    });

    // One-shot auto-fit: CAD defaults frame a ~10m scene, but most .ri files
    // are small (mm-scale). Without an auto-fit, the model is invisible on
    // first open. Fire once when the first non-empty mesh state arrives.
    let hasAutoFit = false;

    // Auto-enable FEA mode on first mesh arrival with non-empty scalar_channels.
    // Guarded by feaModeStore presence; tryAutoEnable enforces one-shot semantics
    // so the effect can fire on every mesh update safely.
    if (props.feaModeStore) {
      const feaStore = props.feaModeStore;
      createEffect(() => {
        const meshes = props.meshes;
        for (const mesh of Object.values(meshes)) {
          if (!mesh.scalar_channels) continue;
          for (const [channel, scalars] of Object.entries(mesh.scalar_channels)) {
            if (scalars.length > 0) {
              feaStore.tryAutoEnable(channel);
              return; // only need the first non-empty channel
            }
          }
        }
      });
    }

    // Sync meshes reactively
    createEffect(() => {
      meshManager.sync(props.meshes);

      // Refresh selection wireframe to reflect updated geometry
      selection.refreshSelected();

      // Adjust camera clipping planes based on scene bounds (include ghost meshes for correct framing)
      const bounds = new Box3();
      for (const mesh of meshManager.getSceneMeshes().values()) {
        bounds.expandByObject(mesh);
      }
      for (const mesh of meshManager.getGhostMeshes().values()) {
        bounds.expandByObject(mesh);
      }
      adjustClipping(bounds);

      if (!hasAutoFit && meshManager.getSceneMeshes().size > 0) {
        selection.fitToView();
        hasAutoFit = true;
      }

      requestRender();
    });

    // Sync entity visibility reactively, resetting removed entities to 'show'.
    // prevVisKeys is intentionally non-reactive (plain Set, not a signal) — it tracks
    // previously-seen keys purely for diff logic. SolidJS does not track it, which is correct:
    // the effect is re-triggered by props.entityVisibility changes, and prevVisKeys is updated
    // synchronously as a side effect. No other code path touches prevVisKeys.
    let prevVisKeys = new Set<string>();
    // COUPLED INVARIANT with meshManager.ts: the `sync()` function in meshManager
    // prunes any visibilityMap entry whose key is absent from the incoming mesh set
    // (orphan-prune block at the end of sync). That prune is safe precisely because
    // this effect re-applies the authoritative `props.entityVisibility` on every
    // reactive tick, so legitimate state is re-set after a prune. The two pieces
    // form a pair: changing orphan-pruning in meshManager requires revisiting this
    // re-application, and vice versa.
    createEffect(() => {
      const visibility = props.entityVisibility ?? {};
      const currentKeys = new Set(Object.keys(visibility));
      for (const [entityPath, state] of Object.entries(visibility)) {
        meshManager.setVisibility(entityPath, state);
      }
      for (const key of prevVisKeys) {
        if (!currentKeys.has(key)) {
          meshManager.setVisibility(key, 'show');
        }
      }
      prevVisKeys = currentKeys;
      requestRender();
    });

    // Sync hover/selection state from props to selection module
    createEffect(() => {
      selection.setHovered(props.hoveredEntity ?? null);
      requestRender();
    });

    createEffect(() => {
      void props.meshes;
      // selectedEntities (multi-select list) takes precedence over scalar selectedEntity
      if (props.selectedEntities !== undefined) {
        selection.setSelected(props.selectedEntities);
      } else {
        selection.setSelected(props.selectedEntity ?? null);
      }
      requestRender();
    });

    // Resize handling via ResizeObserver
    const resizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width: w, height: h } = entry.contentRect;
        if (w > 0 && h > 0) {
          resize(w, h);
          selection.invalidateRect();
          requestRender();
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
      if (needsRender) {
        renderer.render(scene, camera);
        needsRender = false;
      }
    }
    animate();

    // Cleanup — set disposed first to guard any in-flight RAF callback
    onCleanup(() => {
      disposed = true;
      cancelAnimationFrame(animationFrameId);
      controls.controls.removeEventListener('change', requestRender);
      controls.controls.removeEventListener('end', persistCamera);
      resizeObserver.disconnect();
      selection.dispose();
      controls.dispose();
      meshManager.dispose();
      renderer.dispose();
      if (window.__REIFY_DEBUG__) {
        // Per-key cleanup — only remove this viewport's entry from the map
        // so sibling viewports that are still mounted survive.
        delete window.__REIFY_DEBUG__.viewports?.[props.viewportId];
        // Clear the legacy single slot only if it still points to us.
        if (window.__REIFY_DEBUG__.viewport?.scene === scene) {
          delete window.__REIFY_DEBUG__.viewport;
        }
      }
    });
  });

  return (
    <div
      ref={containerRef}
      data-testid="viewport-container"
      style={{ width: '100%', height: '100%', position: 'relative' }}
      onMouseMove={(e) => {
        const rect = containerRef.getBoundingClientRect();
        setPointerPos({
          x: e.clientX - rect.left,
          y: e.clientY - rect.top,
        });
      }}
    >
      <canvas ref={canvasRef} data-testid="viewport-canvas" tabindex="0" aria-label="3D viewport" />

      {/* Tooltip overlay */}
      <Show when={props.hoveredEntity}>
        <div
          data-testid="viewport-tooltip"
          style={{
            position: 'absolute',
            top: `${pointerPos().y + 16}px`,
            left: `${pointerPos().x + 16}px`,
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

      {/* FEA-mode toolbar — top-right overlay, only when feaModeStore is provided */}
      <Show when={props.feaModeStore}>
        <FeaModeToolbar
          store={props.feaModeStore!}
          onLockCurrent={() => {
            // TODO: compute min/max from active mesh scalar channels (follow-up task)
          }}
        />
      </Show>

      {/* Grid toggle button */}
      <button
        data-testid="toggle-grid"
        onClick={() => setShowGrid((v) => !v)}
        style={{
          position: 'absolute',
          bottom: '12px',
          right: '72px',
          padding: '6px 10px',
          'background-color': 'var(--reify-surface, #2a2a3a)',
          color: 'var(--reify-text, #cdd6f4)',
          border: '1px solid var(--reify-border, #45475a)',
          'border-radius': '4px',
          cursor: 'pointer',
          'font-size': '12px',
          'z-index': '10',
          opacity: showGrid() ? '1' : '0.5',
        }}
        title="Toggle grid and axes"
      >
        Grid
      </button>

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
