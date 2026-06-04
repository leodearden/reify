import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup } from '@solidjs/testing-library';
import { MenuBar } from '../panels/MenuBar';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
vi.mock('three', () => ({ Box3: class { expandByObject() {} isEmpty() { return true; } }, Vector3: class {} }));
vi.mock('html-to-image', () => ({ toPng: vi.fn().mockResolvedValue('data:image/png;base64,STUB') }));

import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { initDebugBridge } from '../debug/bridge';

function makeStores() {
  return {
    engine: { state: { meshes: {}, values: {}, constraints: {}, evalStatus: { phase: 'idle' } }, initFromState: vi.fn() },
    editor: { state: { openFiles: [], activeFile: null, dirtyFiles: [], externallyChanged: [], cursorPosition: null }, openFile: vi.fn() },
    selection: { state: { selectedEntity: null, selectedEntities: [], anchorEntity: null, hoveredEntity: null, highlightedParams: [] }, selectEntity: vi.fn(), hoverEntity: vi.fn() },
    claude: { state: { messages: [], sessionStatus: 'idle', currentMessageId: null } },
    viewState: { resetToDefaultView: vi.fn() },
    layout: { state: { editorWidth: 300, sideWidth: 300, designTreeHeight: 160, propertyHeight: 200, constraintHeight: 140 } },
  } as any;
}

type Handler = (e: { payload: { id: number; command: string; params: Record<string, unknown> } }) => Promise<void>;

describe('mini debug open_menu', () => {
  let capturedHandler: Handler | undefined;

  afterEach(() => {
    cleanup();
    delete window.__REIFY_DEBUG__;
  });

  it('dispatching open_menu via handler works', async () => {
    vi.clearAllMocks();
    vi.mocked(listen).mockImplementation(async (_event, handler: any) => {
      capturedHandler = handler;
      return () => {};
    });

    const stores = makeStores();
    await initDebugBridge(stores);
    render(() => <MenuBar />);

    const trigger = document.querySelector('[data-testid="menu-trigger-file"]');
    console.log('trigger before dispatch:', trigger ? 'FOUND' : 'NOT FOUND');
    console.log('ctx.menuBar:', (window as any).__REIFY_DEBUG__?.menuBar ? 'SET' : 'NOT SET');

    vi.mocked(invoke).mockClear();
    await capturedHandler!({ payload: { id: 1, command: 'open_menu', params: { name: 'file' } } });

    const calls = vi.mocked(invoke).mock.calls;
    const resp = calls.find(c => c[0] === 'debug_response');
    console.log('invoke calls:', calls.length);
    console.log('response call:', resp ? JSON.parse((resp[1] as any).result) : 'NOT FOUND');

    expect(resp).toBeDefined();
    const result = JSON.parse((resp![1] as any).result);
    expect(result.ok).toBe(true);
  });
});
