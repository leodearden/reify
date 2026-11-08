import { describe, it, expect } from 'vitest';
import {
  createLauncherStore,
  initialFileLoadFailureInfo,
  isDocumentLoaded,
  LAUNCH_ERROR_CODES,
} from '../stores/launcherStore';
import type { RecentEntry } from '../types';

const RECENT: RecentEntry[] = [
  { path: '/designs/bracket.ri', opened_at: 2000 },
  { path: '/designs/flange.ri', opened_at: 1000 },
];

describe('launcherStore', () => {
  it('starts with no document error, no recent entries, and the assistant mode selected', () => {
    const store = createLauncherStore();

    expect(store.state).toEqual({
      document: { error: null, recent: [] },
      mode: { selected: 'assistant', debugPort: null, error: null },
    });
  });

  it('applyModeState records the selected mode and the live debug port, clearing a mode error', () => {
    const store = createLauncherStore();
    store.failMode({ code: LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED, message: 'bind refused' });

    store.applyModeState({ mode: 'debug', debug_port: 3939 });

    expect(store.state.mode).toEqual({ selected: 'debug', debugPort: 3939, error: null });
  });

  it('failDocument sets the document error and clearDocumentError clears it', () => {
    const store = createLauncherStore();

    store.failDocument({ code: LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED, message: 'm' });
    expect(store.state.document.error).toEqual({ code: 'E_GUI_OPEN_FAILED', message: 'm' });

    store.clearDocumentError();
    expect(store.state.document.error).toBeNull();
  });

  it('failMode sets the mode error and leaves the selected mode unchanged', () => {
    const store = createLauncherStore();
    store.applyModeState({ mode: 'design', debug_port: null });

    store.failMode({ code: LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED, message: 'bind refused' });

    expect(store.state.mode).toEqual({
      selected: 'design',
      debugPort: null,
      error: { code: 'E_GUI_DEBUG_LISTENER_FAILED', message: 'bind refused' },
    });
  });

  it('setRecent replaces the recent list', () => {
    const store = createLauncherStore();
    store.setRecent([{ path: '/old.ri', opened_at: 1 }]);

    store.setRecent(RECENT);

    expect(store.state.document.recent).toEqual(RECENT);
  });

  it('reset clears both errors and keeps the recent list and the mode', () => {
    const store = createLauncherStore();
    store.setRecent(RECENT);
    store.applyModeState({ mode: 'debug', debug_port: 4321 });
    store.failDocument({ code: LAUNCH_ERROR_CODES.E_GUI_TEMPLATE_WRITE_FAILED, message: 'exists' });
    store.failMode({ code: LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED, message: 'bind refused' });

    store.reset();

    expect(store.state).toEqual({
      document: { error: null, recent: RECENT },
      mode: { selected: 'debug', debugPort: 4321, error: null },
    });
  });

  it('names exactly the three launch error codes, each spelled as its own key', () => {
    expect(Object.entries(LAUNCH_ERROR_CODES)).toEqual([
      ['E_GUI_OPEN_FAILED', 'E_GUI_OPEN_FAILED'],
      ['E_GUI_TEMPLATE_WRITE_FAILED', 'E_GUI_TEMPLATE_WRITE_FAILED'],
      ['E_GUI_DEBUG_LISTENER_FAILED', 'E_GUI_DEBUG_LISTENER_FAILED'],
    ]);
    expect(Object.isFrozen(LAUNCH_ERROR_CODES)).toBe(true);
  });
});

describe('isDocumentLoaded', () => {
  it('is false when the editor has no open file', () => {
    expect(isDocumentLoaded({ openFiles: [] })).toBe(false);
  });

  it('is true when the editor has an open file', () => {
    expect(isDocumentLoaded({ openFiles: [{ path: '/designs/bracket.ri' }] })).toBe(true);
  });
});

describe('initialFileLoadFailureInfo', () => {
  it('presents a launch-file failure as E_GUI_OPEN_FAILED naming the file and the error', () => {
    const info = initialFileLoadFailureInfo({
      path: '/designs/broken.ri',
      error: 'Error reading /designs/broken.ri: No such file',
    });

    expect(info.code).toBe(LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED);
    expect(info.message).toContain('/designs/broken.ri');
    expect(info.message).toContain('No such file');
  });

  it('names the file even when the error does not', () => {
    const info = initialFileLoadFailureInfo({ path: '/designs/broken.ri', error: 'Parse errors: x' });

    expect(info.code).toBe(LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED);
    expect(info.message).toContain('/designs/broken.ri');
    expect(info.message).toContain('Parse errors: x');
  });
});
