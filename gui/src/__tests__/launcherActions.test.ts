import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createLauncherActions, LaunchFailure, type LauncherDeps } from '../app/launcherActions';
import { createLauncherStore, LAUNCH_ERROR_CODES, type LauncherStore } from '../stores/launcherStore';
import type { RecentEntry } from '../types';

const RECENT: RecentEntry[] = [
  { path: '/designs/bracket.ri', opened_at: 2000 },
  { path: '/designs/flange.ri', opened_at: 1000 },
];

function makeDeps() {
  return {
    loadDocument: vi.fn<LauncherDeps['loadDocument']>().mockResolvedValue(undefined),
    createNewFile: vi
      .fn<LauncherDeps['createNewFile']>()
      .mockResolvedValue({ path: '/designs/created.ri', content: '// New design\n' }),
    recordRecent: vi.fn<LauncherDeps['recordRecent']>().mockResolvedValue(RECENT),
    listRecent: vi.fn<LauncherDeps['listRecent']>().mockResolvedValue(RECENT),
    setMode: vi
      .fn<LauncherDeps['setMode']>()
      .mockImplementation(async (mode) => ({ mode, debug_port: mode === 'debug' ? 3939 : null })),
    showAssistant: vi.fn<LauncherDeps['showAssistant']>(),
    ensureDebugBridge: vi.fn<LauncherDeps['ensureDebugBridge']>().mockResolvedValue(undefined),
    notify: vi.fn<LauncherDeps['notify']>(),
  };
}

async function failureOf(promise: Promise<unknown>): Promise<LaunchFailure> {
  const outcome = await promise.then(
    () => undefined,
    (err: unknown) => err,
  );
  expect(outcome).toBeInstanceOf(LaunchFailure);
  return outcome as LaunchFailure;
}

let store: LauncherStore;
let deps: ReturnType<typeof makeDeps>;

beforeEach(() => {
  store = createLauncherStore();
  deps = makeDeps();
});

describe('openDocument', () => {
  it('loads the document, records it as recent and stores the returned list', async () => {
    const actions = createLauncherActions(store, deps);

    await actions.openDocument('/designs/bracket.ri');

    expect(deps.loadDocument).toHaveBeenCalledWith('/designs/bracket.ri');
    expect(deps.recordRecent).toHaveBeenCalledWith('/designs/bracket.ri');
    expect(deps.loadDocument.mock.invocationCallOrder[0]).toBeLessThan(
      deps.recordRecent.mock.invocationCallOrder[0],
    );
    expect(store.state.document.recent).toEqual(RECENT);
  });

  it('clears a prior document error', async () => {
    store.failDocument({ code: LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED, message: 'earlier' });
    const actions = createLauncherActions(store, deps);

    await actions.openDocument('/designs/bracket.ri');

    expect(store.state.document.error).toBeNull();
  });

  it('rejects with E_GUI_OPEN_FAILED carrying the backend message when the load fails, recording nothing', async () => {
    deps.loadDocument.mockRejectedValue('No such file: /designs/missing.ri');
    const actions = createLauncherActions(store, deps);

    const failure = await failureOf(actions.openDocument('/designs/missing.ri'));

    expect(failure.code).toBe(LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED);
    expect(failure.message).toBe('No such file: /designs/missing.ri');
    expect(failure.toInfo()).toEqual({ code: failure.code, message: failure.message });
    expect(deps.recordRecent).not.toHaveBeenCalled();
  });

  it('still resolves when recording the recent entry fails after a successful load, and notifies', async () => {
    deps.recordRecent.mockRejectedValue(new Error('app-data dir is read-only'));
    const actions = createLauncherActions(store, deps);

    await expect(actions.openDocument('/designs/bracket.ri')).resolves.toBeUndefined();

    expect(deps.notify).toHaveBeenCalledTimes(1);
    expect(store.state.document.error).toBeNull();
  });
});

describe('createDocument', () => {
  it('rejects with E_GUI_TEMPLATE_WRITE_FAILED and loads nothing when the template write fails', async () => {
    deps.createNewFile.mockRejectedValue('File already exists: /designs/taken.ri');
    const actions = createLauncherActions(store, deps);

    const failure = await failureOf(actions.createDocument('/designs/taken.ri'));

    expect(failure.code).toBe(LAUNCH_ERROR_CODES.E_GUI_TEMPLATE_WRITE_FAILED);
    expect(deps.loadDocument).not.toHaveBeenCalled();
    expect(deps.recordRecent).not.toHaveBeenCalled();
  });

  it('opens the canonical path the backend returned and records it as recent', async () => {
    const actions = createLauncherActions(store, deps);

    await actions.createDocument('/designs/created');

    expect(deps.createNewFile).toHaveBeenCalledWith('/designs/created');
    expect(deps.loadDocument).toHaveBeenCalledWith('/designs/created.ri');
    expect(deps.recordRecent).toHaveBeenCalledWith('/designs/created.ri');
    expect(store.state.document.recent).toEqual(RECENT);
  });

  it('rejects with E_GUI_OPEN_FAILED when the created file fails to load', async () => {
    deps.loadDocument.mockRejectedValue(new Error('engine refused'));
    const actions = createLauncherActions(store, deps);

    const failure = await failureOf(actions.createDocument('/designs/created.ri'));

    expect(failure.code).toBe(LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED);
  });
});

describe('refreshRecent', () => {
  it('stores the persisted recent list', async () => {
    const actions = createLauncherActions(store, deps);

    await actions.refreshRecent();

    expect(deps.listRecent).toHaveBeenCalledTimes(1);
    expect(store.state.document.recent).toEqual(RECENT);
  });
});

describe('selectMode', () => {
  it('assistant: selects the mode and opens the assistant', async () => {
    store.applyModeState({ mode: 'design', debug_port: null });
    const actions = createLauncherActions(store, deps);

    await actions.selectMode('assistant');

    expect(deps.setMode).toHaveBeenCalledWith('assistant');
    expect(store.state.mode.selected).toBe('assistant');
    expect(deps.showAssistant).toHaveBeenCalledWith(true);
    expect(deps.ensureDebugBridge).not.toHaveBeenCalled();
  });

  it('design: selects the mode and closes the assistant', async () => {
    const actions = createLauncherActions(store, deps);

    await actions.selectMode('design');

    expect(deps.setMode).toHaveBeenCalledWith('design');
    expect(store.state.mode.selected).toBe('design');
    expect(deps.showAssistant).toHaveBeenCalledWith(false);
  });

  it('debug: records the live port and ensures the debug bridge on every selection', async () => {
    const actions = createLauncherActions(store, deps);

    await actions.selectMode('debug');

    expect(store.state.mode).toEqual({ selected: 'debug', debugPort: 3939, error: null });
    expect(deps.ensureDebugBridge).toHaveBeenCalledTimes(1);

    await actions.selectMode('debug');

    expect(deps.setMode).toHaveBeenCalledTimes(2);
    expect(deps.ensureDebugBridge).toHaveBeenCalledTimes(2);
  });

  it('debug: rejects with E_GUI_DEBUG_LISTENER_FAILED when the debug bridge fails to start', async () => {
    deps.ensureDebugBridge.mockRejectedValue(new Error('Failed to fetch dynamically imported module'));
    const actions = createLauncherActions(store, deps);

    const failure = await failureOf(actions.selectMode('debug'));

    expect(failure.code).toBe(LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED);
    expect(failure.message).toContain('Failed to fetch dynamically imported module');
    expect(store.state.mode.debugPort).toBe(3939);
  });

  it('debug: two selections in flight together both settle with the one bridge start', async () => {
    let failStart: (err: unknown) => void = () => {};
    const sharedStart = new Promise<void>((_, reject) => {
      failStart = reject;
    });
    deps.ensureDebugBridge.mockReturnValue(sharedStart);
    const actions = createLauncherActions(store, deps);
    const settled: string[] = [];

    const first = failureOf(actions.selectMode('debug')).then((f) => settled.push(f.code));
    const second = failureOf(actions.selectMode('debug')).then((f) => settled.push(f.code));
    await vi.waitFor(() => expect(deps.ensureDebugBridge).toHaveBeenCalledTimes(2));
    expect(settled).toEqual([]);

    failStart(new Error('listen failed'));
    await Promise.all([first, second]);

    expect(settled).toEqual([
      LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED,
      LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED,
    ]);
  });

  it('rejects with E_GUI_DEBUG_LISTENER_FAILED, keeps the selection and starts no bridge when set_mode fails', async () => {
    deps.setMode.mockRejectedValue('Address already in use (os error 98)');
    const actions = createLauncherActions(store, deps);

    const failure = await failureOf(actions.selectMode('debug'));

    expect(failure.code).toBe(LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED);
    expect(store.state.mode.selected).toBe('assistant');
    expect(deps.ensureDebugBridge).not.toHaveBeenCalled();
    expect(deps.showAssistant).not.toHaveBeenCalled();
  });
});
