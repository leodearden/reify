import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { PersistentViewState } from '../types';

// Mock bridge module so tests run without Tauri runtime.
vi.mock('../bridge', () => ({
  readViewSidecar: vi.fn(),
  writeViewSidecar: vi.fn(),
}));

import { readViewSidecar, writeViewSidecar } from '../bridge';
import { loadSidecar, saveSidecar } from '../stores/sidecarPersistence';

const mockReadViewSidecar = vi.mocked(readViewSidecar);
const mockWriteViewSidecar = vi.mocked(writeViewSidecar);

const validState: PersistentViewState = {
  version: '2',
  activeViewId: 'auto:default',
  userViews: [],
  explicit: {},
  viewportCameras: {},
  timestamp: '2026-01-01T00:00:00Z',
};

beforeEach(() => {
  vi.clearAllMocks();
});

describe('loadSidecar', () => {
  it('returns null when bridge returns null', async () => {
    mockReadViewSidecar.mockResolvedValue(null);

    const result = await loadSidecar('/project/bracket.ri');

    expect(mockReadViewSidecar).toHaveBeenCalledWith('/project/bracket.ri');
    expect(result).toBeNull();
  });

  it('returns parsed PersistentViewState when bridge returns a valid payload', async () => {
    mockReadViewSidecar.mockResolvedValue(validState);

    const result = await loadSidecar('/project/bracket.ri');

    expect(result).toEqual(validState);
  });

  it('returns null when bridge returns a legacy v1 payload — version is the sole differentiator (Task 3233)', async () => {
    // Construct the same payload twice, differing only in the `version` field.
    // The positive case (v2) must load; the negative case (v1) must be rejected.
    // This pins the rejection to the version field specifically, making the test
    // resilient to unrelated schema additions.
    const sharedPayload = {
      activeViewId: 'auto:default',
      userViews: [],
      explicit: {},
      viewportCameras: { 'design-main': { position: [0, 10, 0], target: [0, 0, 0], up: [0, 1, 0], zoom: 1 } },
      timestamp: '2026-04-22T00:00:00.000Z',
    };

    // v2 — same data, must load successfully
    const v2Payload = { version: '2', ...sharedPayload } as unknown as PersistentViewState;
    mockReadViewSidecar.mockResolvedValue(v2Payload);
    expect(await loadSidecar('/project/bracket.ri')).not.toBeNull();

    // v1 — identical payload, only version differs; must be rejected
    const legacyPayload = { version: '1', ...sharedPayload } as unknown as PersistentViewState;
    mockReadViewSidecar.mockResolvedValue(legacyPayload);
    expect(await loadSidecar('/project/bracket.ri')).toBeNull();
  });

  it('returns null when payload fails shape validation (defensive guard)', async () => {
    // Bridge returns a payload that looks JSON-valid but is missing required fields
    // (simulates wire-format drift between the TS type and what the Rust backend sends).
    const malformedPayload = {
      // Missing 'version', 'explicit', 'viewportCameras', 'timestamp'
      activeViewId: 'auto:default',
      userViews: [],
    } as unknown as PersistentViewState;

    mockReadViewSidecar.mockResolvedValue(malformedPayload);

    const result = await loadSidecar('/project/bracket.ri');

    expect(result).toBeNull();
  });
});

describe('saveSidecar', () => {
  it('calls bridge.writeViewSidecar with riPath and state', async () => {
    mockWriteViewSidecar.mockResolvedValue(undefined);

    await saveSidecar('/project/bracket.ri', validState);

    expect(mockWriteViewSidecar).toHaveBeenCalledWith('/project/bracket.ri', validState);
  });

  it('resolves without error on success', async () => {
    mockWriteViewSidecar.mockResolvedValue(undefined);

    await expect(saveSidecar('/project/bracket.ri', validState)).resolves.toBeUndefined();
  });

  it('rejects when bridge.writeViewSidecar rejects', async () => {
    mockWriteViewSidecar.mockRejectedValue(new Error('write failed'));

    await expect(saveSidecar('/project/bracket.ri', validState)).rejects.toThrow('write failed');
  });
});
