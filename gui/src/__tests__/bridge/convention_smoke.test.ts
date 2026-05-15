/**
 * §8.2 boundary tests for the GR-016 β convention helpers.
 *
 * This file is the §8.2 boundary-test landing for GR-016 β. It exercises the
 * convention pattern documented in `docs/prds/v0_3/gui-event-channel-inventory.md`
 * §3.5/§6.3 using a synthetic 'convention-smoke' channel not present in any
 * production bridge module (per the "fixture not tied to any production channel"
 * requirement in PRD §9 task β).
 *
 * The inline `onConventionSmoke` wrapper defined below demonstrates the §3.5
 * per-channel pattern that Phase 2/3 tasks (δ, ε, ζ, η, θ) will follow for
 * real channels.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { listen } from '@tauri-apps/api/event';

// Must be declared at module scope before any imports from mockEvents.ts —
// matches the established pattern in bridge.test.ts:9, claudeBridge.test.ts:8, etc.
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import { mockTauriEvent } from '../test_utils/mockEvents';
import { validatePayload } from '../../bridge';

// ── Convention smoke fixture ─────────────────────────────────────────────────

/** KEYS_* hoisted at module level — avoids per-call allocations (§3.5 rule 1). */
const KEYS_CONVENTION_SMOKE: string[] = ['id', 'label'];

/**
 * Inline per-channel wrapper demonstrating the §3.5 pattern.
 * Lives in the test file, NOT in production bridge.ts — this is a fixture
 * for a synthetic channel, not a production event subscription.
 */
async function onConventionSmoke(
  cb: (payload: Record<string, unknown>) => void,
): Promise<() => void> {
  return listen<unknown>('convention-smoke', (event) => {
    const p = validatePayload('convention-smoke', event.payload, KEYS_CONVENTION_SMOKE);
    if (p) cb(p);
  });
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('convention smoke (GR-016 β)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset the channel to clear any handlers registered in previous tests.
    mockTauriEvent('convention-smoke').reset();
  });

  it('typed listen happy-path: callback fires with valid payload', async () => {
    const smokeHandle = mockTauriEvent<{ id: string; label: string }>('convention-smoke');
    const cb = vi.fn();

    await onConventionSmoke(cb);
    smokeHandle.emit({ id: 'test-1', label: 'hello' });

    expect(cb).toHaveBeenCalledOnce();
    expect(cb).toHaveBeenCalledWith({ id: 'test-1', label: 'hello' });
  });
});
