/**
 * Panel rendering tests for `WarmPoolDebugPanel` (GR-016 ε, step-12).
 *
 * Tests the debug panel that subscribes to `warm-pool-event` on mount,
 * tracks evict/donate counts via Solid signals, and displays the live
 * counts and most-recent node_id.
 *
 * Pattern mirrors AutoResolvePanel.test.tsx for render helpers and
 * convention_smoke.test.ts for the mockTauriEvent fixture setup.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, cleanup } from '@solidjs/testing-library';
import { listen } from '@tauri-apps/api/event';

// Must be declared at module scope before any imports that use mockEvents.ts.
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import { mockTauriEvent, clearAllMockEvents } from './test_utils/mockEvents';
import { WarmPoolDebugPanel } from '../debug/WarmPoolDebugPanel';
import type { WarmPoolEvent } from '../types';

// ── Setup / teardown ─────────────────────────────────────────────────────────

beforeEach(() => {
  vi.mocked(listen).mockReset();
  clearAllMockEvents();
  vi.clearAllMocks();
});

afterEach(() => {
  cleanup();
});

// ── Test group (a): initial render ───────────────────────────────────────────

describe('WarmPoolDebugPanel (a) initial render', () => {
  it('(a.1) renders with evicted count of 0', async () => {
    mockTauriEvent<WarmPoolEvent>('warm-pool-event');
    render(() => <WarmPoolDebugPanel />);
    // Evicted count starts at 0
    expect(screen.getByTestId('warm-pool-evicted-count').textContent).toBe('0');
  });

  it('(a.2) renders with donated count of 0', async () => {
    mockTauriEvent<WarmPoolEvent>('warm-pool-event');
    render(() => <WarmPoolDebugPanel />);
    // Donated count starts at 0
    expect(screen.getByTestId('warm-pool-donated-count').textContent).toBe('0');
  });

  it('(a.3) renders the panel container with data-testid="warm-pool-debug-panel"', async () => {
    mockTauriEvent<WarmPoolEvent>('warm-pool-event');
    render(() => <WarmPoolDebugPanel />);
    expect(screen.getByTestId('warm-pool-debug-panel')).toBeTruthy();
  });
});

// ── Test group (b): donated event increments donated count ───────────────────

describe('WarmPoolDebugPanel (b) donated event', () => {
  it('(b.1) donated count increments to 1 after one donated event', async () => {
    const handle = mockTauriEvent<WarmPoolEvent>('warm-pool-event');
    render(() => <WarmPoolDebugPanel />);

    handle.emit({ kind: 'donated', size_bytes: 1024, node_id: 'A.width' });

    expect(screen.getByTestId('warm-pool-donated-count').textContent).toBe('1');
    expect(screen.getByTestId('warm-pool-evicted-count').textContent).toBe('0');
  });

  it('(b.2) donated count increments to 3 after three donated events', async () => {
    const handle = mockTauriEvent<WarmPoolEvent>('warm-pool-event');
    render(() => <WarmPoolDebugPanel />);

    handle.emit({ kind: 'donated', size_bytes: 512, node_id: 'A.x' });
    handle.emit({ kind: 'donated', size_bytes: 512, node_id: 'B.y' });
    handle.emit({ kind: 'donated', size_bytes: 512, node_id: 'C.z' });

    expect(screen.getByTestId('warm-pool-donated-count').textContent).toBe('3');
  });
});

// ── Test group (c): evicted events increment evicted count ───────────────────

describe('WarmPoolDebugPanel (c) evicted events', () => {
  it('(c.1) two evicted events bump evicted count to 2', async () => {
    const handle = mockTauriEvent<WarmPoolEvent>('warm-pool-event');
    render(() => <WarmPoolDebugPanel />);

    handle.emit({ kind: 'evicted', size_bytes: 2048, node_id: 'Body.thickness' });
    handle.emit({ kind: 'evicted', size_bytes: 4096, node_id: 'Plate.width' });

    expect(screen.getByTestId('warm-pool-evicted-count').textContent).toBe('2');
    expect(screen.getByTestId('warm-pool-donated-count').textContent).toBe('0');
  });
});

// ── Test group (d): most-recent node_id ─────────────────────────────────────

describe('WarmPoolDebugPanel (d) most-recent node_id', () => {
  it('(d.1) displays the node_id of the most recently received event', async () => {
    const handle = mockTauriEvent<WarmPoolEvent>('warm-pool-event');
    render(() => <WarmPoolDebugPanel />);

    handle.emit({ kind: 'evicted', size_bytes: 1024, node_id: 'Body.thickness' });
    handle.emit({ kind: 'donated', size_bytes: 2048, node_id: 'Plate.width' });

    // The last node_id ('Plate.width') should be shown
    expect(screen.getByTestId('warm-pool-last-node-id').textContent).toBe('Plate.width');
  });

  it('(d.2) node_id display is null/empty before any event arrives', async () => {
    mockTauriEvent<WarmPoolEvent>('warm-pool-event');
    render(() => <WarmPoolDebugPanel />);

    // Before any events, no last-node-id text is shown (empty string or absent)
    const el = screen.queryByTestId('warm-pool-last-node-id');
    // Either the element isn't rendered, or its content is empty
    expect(el === null || el.textContent === '' || el.textContent === null).toBe(true);
  });
});

// ── Test group (e): unlisten on unmount ──────────────────────────────────────

describe('WarmPoolDebugPanel (e) unlisten on unmount', () => {
  it('(e.1) unlisten spy is called after unmount (lifecycle contract pinned)', async () => {
    // Directly capture the UnlistenFn returned by listen so we can assert it was
    // invoked by onCleanup.  Using DOM-teardown alone (queryByTestId → null) does NOT
    // prove unlisten was called — a panel that forgets onCleanup would still pass that
    // weaker assertion.  This test pins the contract directly.
    const unlistenSpy = vi.fn();
    vi.mocked(listen).mockImplementation(async () => unlistenSpy);

    const { unmount } = render(() => <WarmPoolDebugPanel />);

    // Flush the async resolution chain.  onMount fires synchronously during render,
    // calling onWarmPoolEvent → listen().  The chain is:
    //   tick 1: listen()'s `async () => unlistenSpy` promise resolves to unlistenSpy
    //   tick 2: onWarmPoolEvent()'s async-function wrapper follows listen()'s promise
    //   tick 3: safety margin for any additional V8 promise-following ticks
    // After these ticks, unlistenPromise (= the Promise returned by onWarmPoolEvent)
    // is fully resolved to unlistenSpy.
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    // listen() must have been called once (component registered for the channel).
    expect(listen).toHaveBeenCalledOnce();
    expect(listen).toHaveBeenCalledWith('warm-pool-event', expect.any(Function));

    unmount();

    // Flush the cleanup chain: onCleanup fires unlistenPromise.then(fn => fn()).
    // Since unlistenPromise is already resolved, fn() is scheduled as a microtask.
    await Promise.resolve();

    // This is the contract: onCleanup must call the UnlistenFn so the Tauri event
    // listener is deregistered.  A panel that omitted onCleanup would leave the spy
    // uncalled and fail here.
    expect(unlistenSpy).toHaveBeenCalledOnce();
  });
});
