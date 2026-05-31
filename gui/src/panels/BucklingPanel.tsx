/**
 * BucklingPanel — mode-shape animator panel. Task ι/3458.
 *
 * Props-driven panel (a `store: BucklingStore` prop) that renders:
 *   - A selectable list of registered mode indices
 *   - Play/pause button
 *   - Amplitude scale slider
 *   - Show-undeformed-overlay checkbox
 *   - A <canvas> for the 3D point-cloud preview (guarded — gracefully skipped
 *     under jsdom which has no WebGL context)
 *
 * The RAF animation loop lives here: each frame calls store.tick(dt) and
 * animator.update(store.currentDisplacedPositions()), keeping phase + scale
 * logic on the frontend (PRD §14 Q4/Q5 resolution).
 */

import { For, Show, onMount, onCleanup, createMemo } from 'solid-js';
import { Scene, PerspectiveCamera, WebGLRenderer } from 'three';
import type { BucklingStore } from '../stores/bucklingStore';
import { createBucklingAnimator, computePointCloudBounds } from '../viewport/bucklingAnimator';
import { computeModeThumbnail } from '../viewport/modeThumbnail';

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/**
 * Format a buckling eigenvalue λ for display (task 4072).
 * Returns '—' for null/undefined/NaN; otherwise toPrecision(4).
 */
export function formatEigenvalue(v: number | null | undefined): string {
  if (v === null || v === undefined || typeof v !== 'number' || !Number.isFinite(v)) {
    return '—';
  }
  return v.toPrecision(4);
}

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

export interface BucklingPanelProps {
  /** The buckling animation store — caller owns lifetime. */
  store: BucklingStore;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

/**
 * BucklingPanel.
 *
 * Renders the DOM controls unconditionally. The 3D point-cloud canvas is
 * initialised inside onMount and guarded by a WebGL context check so that
 * the component renders correctly under jsdom (no WebGL).
 */
export function BucklingPanel(props: BucklingPanelProps) {
  const { store } = props;
  let canvasRef: HTMLCanvasElement | undefined;

  onMount(() => {
    if (!canvasRef) return;

    // Guard: jsdom and non-WebGL environments return null here.
    // Probe webgl2 before webgl so Three's WebGLRenderer can still acquire the
    // webgl2 context; probing 'webgl' first permanently commits this canvas to
    // WebGL1 (per spec, a canvas cannot switch context type once acquired).
    let hasWebGL: boolean;
    try {
      hasWebGL = !!(canvasRef.getContext('webgl2') ?? canvasRef.getContext('webgl'));
    } catch (_) {
      hasWebGL = false;
    }
    if (!hasWebGL) return;

    // 3D initialisation — only reached in a real WebGL environment.
    const base = store.state.base;
    if (!base) return;

    const animator = createBucklingAnimator(base);

    // Build a self-contained mini-scene for the panel canvas.
    const scene = new Scene();
    scene.add(animator.object3d);
    scene.add(animator.undeformedOverlay);

    const { center, radius } = computePointCloudBounds(base);
    const d = radius > 0 ? radius * 3 : 1;
    const camera = new PerspectiveCamera(60, canvasRef.width / canvasRef.height, 0.1, 10000);
    camera.up.set(0, 0, 1); // Z-up — matches kernel convention
    camera.position.set(center[0] + d, center[1] + d, center[2] + d);
    camera.lookAt(center[0], center[1], center[2]);
    camera.updateProjectionMatrix();

    const renderer = new WebGLRenderer({ canvas: canvasRef, antialias: true });
    renderer.setSize(canvasRef.width, canvasRef.height);

    let rafId: number;
    let lastTime: number | null = null;

    function frame(now: number) {
      const dt = lastTime !== null ? now - lastTime : 0;
      lastTime = now;
      store.tick(dt);
      const positions = store.currentDisplacedPositions();
      if (positions) animator.update(positions);
      animator.setUndeformedVisible(store.state.showUndeformed);
      renderer.render(scene, camera);
      rafId = requestAnimationFrame(frame);
    }

    rafId = requestAnimationFrame(frame);

    onCleanup(() => {
      cancelAnimationFrame(rafId);
      animator.dispose();
      renderer.dispose();
    });
  });

  // ── Render ───────────────────────────────────────────────────────────────

  return (
    <div data-testid="buckling-panel" style={{ display: 'flex', 'flex-direction': 'column', gap: '6px', padding: '8px' }}>
      {/* Mode list */}
      <div>
        <div style={{ 'font-size': '12px', 'font-weight': '600', 'margin-bottom': '4px' }}>Modes</div>
        <Show
          when={store.modes().length > 0}
          fallback={<div style={{ 'font-size': '11px', color: '#888' }}>No buckling modes available</div>}
        >
          <For each={store.modes()}>
            {(modeIdx) => {
              const m = () => store.state.modes[String(modeIdx)];
              const tn = createMemo(() => {
                const base = store.state.base;
                const peak = m()?.peak;
                if (!base || !peak || base.length === 0 || peak.length === 0) return null;
                return computeModeThumbnail(base, peak);
              });
              return (
                <div
                  data-testid={`buckling-mode-row-${modeIdx}`}
                  role="button"
                  tabIndex={0}
                  style={{
                    cursor: 'pointer',
                    padding: '3px 6px',
                    'border-radius': '3px',
                    display: 'flex',
                    'align-items': 'center',
                    gap: '6px',
                    background: store.state.selectedMode === modeIdx ? 'rgba(68,136,255,0.25)' : 'transparent',
                    'font-size': '12px',
                  }}
                  onClick={() => store.selectMode(modeIdx)}
                  onKeyDown={(e) => { if (e.key === 'Enter') store.selectMode(modeIdx); }}
                >
                  <Show when={tn() !== null}>
                    <svg
                      data-testid={`buckling-mode-thumbnail-${modeIdx}`}
                      viewBox={tn()!.viewBox}
                      width="24"
                      height="24"
                      style={{ flex: 'none', border: '1px solid #555', 'border-radius': '2px' }}
                    >
                      <For each={tn()!.points}>
                        {([x, y]) => <circle cx={x} cy={y} r="0.04" fill="#4488ff" />}
                      </For>
                    </svg>
                  </Show>
                  Mode {modeIdx + 1} · λ = {formatEigenvalue(m()?.eigenvalue)}
                </div>
              );
            }}
          </For>
        </Show>
      </div>

      {/* Animation controls */}
      <div style={{ display: 'flex', 'align-items': 'center', gap: '6px' }}>
        <button
          data-testid="buckling-play-pause"
          onClick={() => store.togglePlay()}
          style={{ 'font-size': '11px' }}
        >
          {store.state.playing ? 'Pause' : 'Play'}
        </button>
      </div>

      {/* Scale slider */}
      <div style={{ display: 'flex', 'align-items': 'center', gap: '6px' }}>
        <label style={{ 'font-size': '11px' }} for="buckling-scale">Scale</label>
        <input
          id="buckling-scale"
          data-testid="buckling-scale-slider"
          type="range"
          min="0"
          max="10"
          step="0.1"
          value={store.state.scale}
          onInput={(e) => store.setScale(parseFloat((e.target as HTMLInputElement).value))}
          style={{ flex: '1' }}
        />
      </div>

      {/* Undeformed overlay toggle */}
      <div style={{ display: 'flex', 'align-items': 'center', gap: '6px' }}>
        <input
          data-testid="buckling-show-undeformed"
          type="checkbox"
          checked={store.state.showUndeformed}
          onClick={() => store.setShowUndeformed(!store.state.showUndeformed)}
          id="buckling-show-undeformed"
        />
        <label style={{ 'font-size': '11px' }} for="buckling-show-undeformed">
          Show undeformed
        </label>
      </div>

      {/* 3D canvas — guarded by WebGL check in onMount */}
      <canvas
        ref={canvasRef}
        width={300}
        height={200}
        style={{ background: '#1a1a1a', 'border-radius': '4px', display: 'block' }}
      />
    </div>
  );
}
