/**
 * WarmPoolDebugPanel — GR-016 ε debug surface.
 *
 * Subscribes to the `warm-pool-event` Tauri channel on mount, tracks evict/donate
 * counts via Solid signals, and displays the live counts and most-recent node_id.
 *
 * Mounted only under the REIFY_DEBUG=1 gate in App.tsx per PRD §11 Q6 resolution:
 * "debug-only initially; promote to always-on if a production GUI surface emerges."
 *
 * Channel spec: docs/gui-event-channels/warm-pool-event.md
 */

import { type Component, createSignal, onCleanup, onMount, Show } from 'solid-js';
import { onWarmPoolEvent } from '../bridge';

/**
 * Debug panel displaying warm-pool evict/donate event counts.
 *
 * Rendered under the `isDebugEnabled()` gate in App.tsx so it only appears in
 * REIFY_DEBUG=1 sessions.
 */
export const WarmPoolDebugPanel: Component = () => {
  const [evictedCount, setEvictedCount] = createSignal(0);
  const [donatedCount, setDonatedCount] = createSignal(0);
  const [lastNodeId, setLastNodeId] = createSignal<string | null>(null);

  onMount(() => {
    const unlistenPromise = onWarmPoolEvent((ev) => {
      if (ev.kind === 'evicted') {
        setEvictedCount((n) => n + 1);
      } else {
        setDonatedCount((n) => n + 1);
      }
      setLastNodeId(ev.node_id);
    });

    onCleanup(() => {
      unlistenPromise.then((fn) => fn());
    });
  });

  return (
    <div
      data-testid="warm-pool-debug-panel"
      style={{
        'font-family': 'monospace',
        'font-size': '11px',
        padding: '4px 8px',
        background: 'rgba(0,0,0,0.6)',
        color: '#ccc',
        'border-radius': '4px',
        'pointer-events': 'none',
      }}
    >
      <div>
        evicted: <span data-testid="warm-pool-evicted-count">{evictedCount()}</span>
        {'  '}donated: <span data-testid="warm-pool-donated-count">{donatedCount()}</span>
      </div>
      <Show when={lastNodeId() !== null}>
        <div>
          last: <span data-testid="warm-pool-last-node-id">{lastNodeId()}</span>
        </div>
      </Show>
    </div>
  );
};
