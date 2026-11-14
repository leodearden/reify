import { describe, it, expect, vi, afterEach } from 'vitest';

afterEach(() => {
  vi.doUnmock('../bridge');
  vi.resetModules();
});

describe('engineStoreBridgeHandles', () => {
  it('binds every handle to the shared bridge mock', async () => {
    vi.resetModules();
    vi.doMock('../bridge', async () =>
      (await import('./engineStoreBridgeMockFactory')).engineStoreBridgeMock(),
    );
    const handles = await import('./engineStoreBridgeHandles');
    const bridge = await import('../bridge');

    expect(handles.mockOnMeshUpdate).toBe(bridge.onMeshUpdate);
    expect(vi.isMockFunction(handles.mockOnEvalGeneration)).toBe(true);
  });

  it('primeEngineStoreBridgeMocks resets every channel subscribeToEvents subscribes to, whatever an earlier test left installed', async () => {
    vi.resetModules();
    vi.doMock('../bridge', async () =>
      (await import('./engineStoreBridgeMockFactory')).engineStoreBridgeMock(),
    );
    const handles = await import('./engineStoreBridgeHandles');
    const { createEngineStore } = await import('../stores/engineStore');

    for (const handle of Object.values(handles)) {
      if (vi.isMockFunction(handle)) {
        handle.mockRejectedValue(new Error('left installed by an earlier test'));
        handle.mockRejectedValueOnce(new Error('queued by an earlier test'));
      }
    }

    handles.primeEngineStoreBridgeMocks();

    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      const cleanup = await createEngineStore().subscribeToEvents();
      expect(warn).not.toHaveBeenCalled();
      expect(() => cleanup()).not.toThrow();
    } finally {
      warn.mockRestore();
    }
  });

  it('refuses to load against the real bridge, naming the vi.mock it needs', async () => {
    vi.resetModules();
    vi.doMock('../bridge', () => vi.importActual('../bridge'));

    await expect(import('./engineStoreBridgeHandles')).rejects.toThrow(
      /vi\.mock\('\.\.\/bridge'/,
    );
  });
});
