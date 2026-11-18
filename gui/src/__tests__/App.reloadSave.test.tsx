import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@solidjs/testing-library';
import { deferred, withSuppressedRejectionsAndErrorSpy } from './test-utils';
import { published } from './test_utils/publishedState';
import { makeGuiState } from './test_utils/guiState';

vi.mock('@tauri-apps/api/core', async () => (await import('./appHarness/moduleMocks')).tauriCoreMock());
vi.mock('@tauri-apps/api/event', async () => (await import('./appHarness/moduleMocks')).tauriEventMock());
vi.mock('../viewport', async () => (await import('./appHarness/renderStubs')).viewportStubModule());
vi.mock('../editor/Editor', async () => (await import('./appHarness/renderStubs')).editorStubModule());
vi.mock('../editor/FileTabs', async () => (await import('./appHarness/renderStubs')).fileTabsStubModule());
vi.mock('../bridge', async () => (await import('./appHarness/moduleMocks')).appBridgeMock());
vi.mock('../debug', async () => (await import('./appHarness/moduleMocks')).debugMock());
vi.mock('../stores/sidecarPersistence', async () => (await import('./appHarness/moduleMocks')).sidecarPersistenceMock());
vi.mock('../stores/viewPersistence', async (importOriginal) => (await import('./appHarness/moduleMocks')).viewPersistenceMock(await importOriginal()));

import App from '../App';
import * as bridge from '../bridge';
import { primeAppHarness } from './appHarness/harness';
import { capturedEditorStore, capturedEditorLiveContentRef } from './appHarness/renderStubs';

beforeEach(primeAppHarness);

describe('App changedFiles multi-file tracking (R-1)', () => {
  const testState = makeGuiState({
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
      { path: '/project/gear.ri', content: 'structure Gear {}' },
    ],
  });

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));
    vi.mocked(bridge.openFile).mockImplementation(async (path: string) => ({
      path,
      content: `updated ${path}`,
    }));
  });

  it('two different file changes show both in reload prompt', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Simulate two different files changing
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    capturedEditorStore.markDirty('/project/gear.ri');
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });

    await waitFor(() => {
      expect(screen.getByText(/2 files changed/)).toBeTruthy();
    });
  });

  it('same file changed twice results in only one entry', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });

    await waitFor(() => {
      expect(screen.getByTestId('reload-prompt')).toBeTruthy();
    });
    // Should show single file, not "2 files changed"
    const reloadPrompt = screen.getByTestId('reload-prompt');
    expect(reloadPrompt.textContent).toMatch(/bracket\.ri/);
    expect(reloadPrompt.textContent).not.toMatch(/2 files changed/);
  });

  it('handleReload reloads all files in the changed set', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    capturedEditorStore.markDirty('/project/gear.ri');
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });

    await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

    capturedEditorStore.markClean('/project/bracket.ri');
    capturedEditorStore.markClean('/project/gear.ri');

    // Click Reload
    fireEvent.click(screen.getByText('Reload'));

    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/bracket.ri');
      expect(bridge.openFile).toHaveBeenCalledWith('/project/gear.ri');
    });
  });

  it('handleDismissReload clears all changed files', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    capturedEditorStore.markDirty('/project/gear.ri');
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });

    await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

    // Click Dismiss
    fireEvent.click(screen.getByText('Dismiss'));

    await waitFor(() => {
      expect(screen.queryByTestId('reload-prompt')).toBeNull();
    });
  });
});

describe('App dirty-file check before reload (R-4)', () => {
  const testState = makeGuiState({
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
      { path: '/project/gear.ri', content: 'structure Gear {}' },
    ],
  });

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));
    vi.mocked(bridge.openFile).mockImplementation(async (path: string) => ({
      path,
      content: `updated ${path}`,
    }));
  });

  it('when no dirty files overlap, handleReload proceeds immediately', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    await waitFor(() => expect(screen.getByTestId('reload-prompt')).toBeTruthy());

    capturedEditorStore.markClean('/project/bracket.ri');

    // No dirty files — Reload should proceed immediately
    fireEvent.click(screen.getByText('Reload'));

    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/bracket.ri');
    });
  });

  it('when dirty files overlap with changed files, shows confirmation warning instead of reloading', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Mark bracket.ri as dirty (unsaved changes)
    capturedEditorStore.markDirty('/project/bracket.ri');

    // Trigger file change for the dirty file
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    await waitFor(() => expect(screen.getByTestId('reload-prompt')).toBeTruthy());

    // Click Reload — should show confirmation warning, NOT call bridgeOpenFile
    fireEvent.click(screen.getByText('Reload'));

    await waitFor(() => {
      expect(screen.getByText(/Unsaved changes will be lost/)).toBeTruthy();
      expect(screen.getByText('Reload Anyway')).toBeTruthy();
    });

    // bridgeOpenFile should NOT have been called
    expect(bridge.openFile).not.toHaveBeenCalled();
  });

  it('confirming Reload Anyway triggers bridgeOpenFile for all changed files', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');

    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    capturedEditorStore.markDirty('/project/gear.ri');
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });
    await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

    // First click shows warning
    fireEvent.click(screen.getByText('Reload'));
    await waitFor(() => expect(screen.getByText('Reload Anyway')).toBeTruthy());

    // Click Reload Anyway — should proceed
    fireEvent.click(screen.getByText('Reload Anyway'));

    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/bracket.ri');
      expect(bridge.openFile).toHaveBeenCalledWith('/project/gear.ri');
    });
  });

  it('dismissing confirmation does not reload', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');

    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    await waitFor(() => expect(screen.getByTestId('reload-prompt')).toBeTruthy());

    // First click shows warning
    fireEvent.click(screen.getByText('Reload'));
    await waitFor(() => expect(screen.getByText('Reload Anyway')).toBeTruthy());

    // Click Dismiss
    fireEvent.click(screen.getByText('Dismiss'));

    await waitFor(() => {
      expect(screen.queryByTestId('reload-prompt')).toBeNull();
    });

    // bridgeOpenFile should NOT have been called
    expect(bridge.openFile).not.toHaveBeenCalled();
  });
});

describe('App handleReload partial failure', () => {
  const testState = makeGuiState({
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
      { path: '/project/gear.ri', content: 'structure Gear {}' },
    ],
  });

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));
  });

  it('when one file succeeds and another fails, only the failed file remains in changedFiles', async () => {
    // bracket.ri succeeds, gear.ri fails
    vi.mocked(bridge.openFile).mockImplementation(async (path: string) => {
      if (path === '/project/gear.ri') {
        throw new Error('disk read error');
      }
      return { path, content: `updated ${path}` };
    });

    await withSuppressedRejectionsAndErrorSpy(async () => {
      render(() => <App />);
      await waitFor(() => expect(fileChangedCallback).toBeDefined());
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      capturedEditorStore.markDirty('/project/bracket.ri');
      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      capturedEditorStore.markDirty('/project/gear.ri');
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

      capturedEditorStore.markClean('/project/bracket.ri');
      capturedEditorStore.markClean('/project/gear.ri');

      fireEvent.click(screen.getByText('Reload'));

      // After allSettled, only gear.ri should remain (bracket.ri succeeded)
      await waitFor(() => {
        const prompt = screen.getByTestId('reload-prompt');
        expect(prompt.textContent).toMatch(/gear\.ri/);
        expect(prompt.textContent).not.toMatch(/bracket\.ri/);
        expect(prompt.textContent).not.toMatch(/2 files changed/);
      });
    });
  });

  it('when one file fails, confirmReload is still reset to false', async () => {
    vi.mocked(bridge.openFile).mockImplementation(async (path: string) => {
      if (path === '/project/gear.ri') {
        throw new Error('disk read error');
      }
      return { path, content: `updated ${path}` };
    });

    await withSuppressedRejectionsAndErrorSpy(async () => {
      render(() => <App />);
      await waitFor(() => expect(fileChangedCallback).toBeDefined());
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      // Mark gear.ri as dirty so we enter confirmReload flow
      capturedEditorStore.markDirty('/project/gear.ri');

      capturedEditorStore.markDirty('/project/bracket.ri');
      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

      // First click shows confirmation warning
      fireEvent.click(screen.getByText('Reload'));
      await waitFor(() => expect(screen.getByText('Reload Anyway')).toBeTruthy());

      // Confirm reload
      fireEvent.click(screen.getByText('Reload Anyway'));

      // After partial failure, confirmReload should be reset
      // (so the remaining gear.ri prompt shows normal Reload, not Reload Anyway)
      await waitFor(() => {
        const prompt = screen.getByTestId('reload-prompt');
        // gear.ri remains but confirmReload was reset — should NOT show 'Reload Anyway'
        expect(prompt.textContent).toMatch(/gear\.ri/);
        expect(prompt.textContent).not.toMatch(/Reload Anyway/);
      });
    });
  });

  it('when one file fails, an error toast is shown mentioning the failure count', async () => {
    vi.mocked(bridge.openFile).mockImplementation(async (path: string) => {
      if (path === '/project/gear.ri') {
        throw new Error('disk read error');
      }
      return { path, content: `updated ${path}` };
    });

    await withSuppressedRejectionsAndErrorSpy(async () => {
      render(() => <App />);
      await waitFor(() => expect(fileChangedCallback).toBeDefined());
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      capturedEditorStore.markDirty('/project/bracket.ri');
      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      capturedEditorStore.markDirty('/project/gear.ri');
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

      capturedEditorStore.markClean('/project/bracket.ri');
      capturedEditorStore.markClean('/project/gear.ri');

      fireEvent.click(screen.getByText('Reload'));

      // An error toast should appear indicating reload failure
      await waitFor(() => {
        const toasts = screen.getAllByTestId('toast');
        const errorToast = toasts.find((t) => t.textContent?.match(/failed.*reload/i));
        expect(errorToast).toBeTruthy();
      });
    });
  });

  it('when all files succeed, changedFiles is cleared completely', async () => {
    vi.mocked(bridge.openFile).mockImplementation(async (path: string) => ({
      path,
      content: `updated ${path}`,
    }));

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    capturedEditorStore.markDirty('/project/gear.ri');
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });
    await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

    capturedEditorStore.markClean('/project/bracket.ri');
    capturedEditorStore.markClean('/project/gear.ri');

    fireEvent.click(screen.getByText('Reload'));

    await waitFor(() => {
      expect(screen.queryByTestId('reload-prompt')).toBeNull();
    });
  });

  it('when all files fail, all files remain in changedFiles', async () => {
    vi.mocked(bridge.openFile).mockImplementation(async (_path: string) => {
      throw new Error('disk error');
    });

    await withSuppressedRejectionsAndErrorSpy(async () => {
      render(() => <App />);
      await waitFor(() => expect(fileChangedCallback).toBeDefined());
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      capturedEditorStore.markDirty('/project/bracket.ri');
      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      capturedEditorStore.markDirty('/project/gear.ri');
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

      capturedEditorStore.markClean('/project/bracket.ri');
      capturedEditorStore.markClean('/project/gear.ri');

      fireEvent.click(screen.getByText('Reload'));

      // After all failures, both files should remain and an error toast should appear
      await waitFor(() => {
        const toasts = screen.getAllByTestId('toast');
        const errorToast = toasts.find((t) => t.textContent?.match(/failed.*reload/i));
        expect(errorToast).toBeTruthy();
      });

      // Both files should still be in changedFiles
      expect(screen.getByText(/2 files changed/)).toBeTruthy();
    });
  });
});

describe('App handleReload race condition', () => {
  const testState = makeGuiState({
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
      { path: '/project/gear.ri', content: 'structure Gear {}' },
      { path: '/project/housing.ri', content: 'structure Housing {}' },
    ],
  });

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));
  });

  it('concurrent file-change event during reload is preserved after all succeed', async () => {
    // Use deferred promises to control when bridgeOpenFile resolves
    const bracketOpen = deferred<any>();
    const gearOpen = deferred<any>();

    vi.mocked(bridge.openFile).mockImplementation((path: string) => {
      if (path === '/project/bracket.ri') return bracketOpen.promise;
      if (path === '/project/gear.ri') return gearOpen.promise;
      return Promise.resolve({ path, content: `updated ${path}` });
    });

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Two files change
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    capturedEditorStore.markDirty('/project/gear.ri');
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });
    await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

    capturedEditorStore.markClean('/project/bracket.ri');
    capturedEditorStore.markClean('/project/gear.ri');

    // Click Reload — starts in-flight reload for bracket.ri and gear.ri
    fireEvent.click(screen.getByText('Reload'));

    // Wait for bridgeOpenFile to be called
    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledTimes(2);
    });

    // DURING the in-flight reload, a new file-change event arrives for housing.ri
    capturedEditorStore.markDirty('/project/housing.ri');
    fileChangedCallback!({ path: '/project/housing.ri', content: '' });

    // Now resolve both promises (both succeed)
    bracketOpen.resolve({ path: '/project/bracket.ri', content: 'updated bracket.ri' });
    gearOpen.resolve({ path: '/project/gear.ri', content: 'updated gear.ri' });

    // After settlement, housing.ri should still be in changedFiles
    // The reload prompt should show housing.ri, not disappear entirely
    await waitFor(() => {
      const prompt = screen.getByTestId('reload-prompt');
      expect(prompt.textContent).toMatch(/housing\.ri/);
    });
  });

  it('concurrent file-change event during reload preserved alongside partial failure', async () => {
    // bracket.ri succeeds, gear.ri fails, housing.ri arrives concurrently
    const bracketOpen = deferred<any>();
    const gearOpen = deferred<any>();

    vi.mocked(bridge.openFile).mockImplementation((path: string) => {
      if (path === '/project/bracket.ri') return bracketOpen.promise;
      if (path === '/project/gear.ri') return gearOpen.promise;
      return Promise.resolve({ path, content: `updated ${path}` });
    });

    await withSuppressedRejectionsAndErrorSpy(async () => {
      render(() => <App />);
      await waitFor(() => expect(fileChangedCallback).toBeDefined());
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      // Two files change
      capturedEditorStore.markDirty('/project/bracket.ri');
      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      capturedEditorStore.markDirty('/project/gear.ri');
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

      capturedEditorStore.markClean('/project/bracket.ri');
      capturedEditorStore.markClean('/project/gear.ri');

      // Click Reload
      fireEvent.click(screen.getByText('Reload'));

      await waitFor(() => {
        expect(bridge.openFile).toHaveBeenCalledTimes(2);
      });

      // Concurrent file-change event during in-flight reload
      capturedEditorStore.markDirty('/project/housing.ri');
      fileChangedCallback!({ path: '/project/housing.ri', content: '' });

      // Resolve bracket (success), reject gear (failure)
      bracketOpen.resolve({ path: '/project/bracket.ri', content: 'updated bracket.ri' });
      gearOpen.reject(new Error('disk error'));

      // After settlement: gear.ri (failed) + housing.ri (concurrent) should both remain
      // bracket.ri (succeeded) should be removed
      await waitFor(() => {
        const prompt = screen.getByTestId('reload-prompt');
        expect(prompt.textContent).toMatch(/2 files changed/);
      });
    });
  });
});

describe('App re-evaluate error toast', () => {
  it('shows error toast when re-evaluate (F5) fails', async () => {
    await withSuppressedRejectionsAndErrorSpy(async (errorSpy) => {
      vi.mocked(bridge.updateSource).mockRejectedValue(new Error('eval error'));
      vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
        files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
      })));

      render(() => <App />);

      // Wait for ready state
      await waitFor(() => {
        expect(screen.getByTestId('app-layout')).toBeTruthy();
      });

      // Press F5 to trigger re-evaluate (on a non-input element)
      fireEvent.keyDown(document, { key: 'F5' });

      // Wait for the error toast to appear
      await waitFor(() => {
        const toastEl = screen.getByTestId('toast');
        expect(toastEl).toBeTruthy();
        expect(toastEl.dataset.type).toBe('error');
        expect(toastEl.textContent).toContain('Re-evaluation failed');
        expect(toastEl.textContent).toContain('eval error');
      });

      // console.error should NOT be called
      expect(errorSpy).not.toHaveBeenCalledWith(
        'Re-evaluate failed:',
        expect.any(Error),
      );
    });
  });
});

describe('App F5 re-evaluate multi-file', () => {
  it('F5 re-evaluate sends only the active file content when multiple files are open', async () => {
    // Arrange: two files — after init, mount.ri is activeFile (last opened)
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      files: [
        { path: '/project/bracket.ri', content: 'structure Bracket {}' },
        { path: '/project/mount.ri', content: 'structure Mount {}' },
      ],
    })));
    vi.mocked(bridge.updateSource).mockResolvedValue(undefined as any);

    render(() => <App />);

    // Wait for ready state (both files opened, activeFile = '/project/mount.ri')
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Modify the non-active file's content via the captured editor store
    capturedEditorStore!.updateFileContent('/project/bracket.ri', 'MODIFIED CONTENT');
    // Also modify the active file's content — F5 should send this updated content, not a stale snapshot
    capturedEditorStore!.updateFileContent('/project/mount.ri', 'structure Mount { updated: true }');

    // Clear any prior updateSource calls (e.g. from initial file load) so the assertion is self-contained
    vi.mocked(bridge.updateSource).mockClear();

    // Act: press F5 to trigger handleReEvaluate
    fireEvent.keyDown(document, { key: 'F5' });

    // Assert: updateSource called exactly once with the ACTIVE file (mount.ri) and its CURRENT content
    await waitFor(() => {
      expect(vi.mocked(bridge.updateSource)).toHaveBeenCalledTimes(1);
    });
    expect(vi.mocked(bridge.updateSource)).toHaveBeenCalledWith(
      '/project/mount.ri',
      'structure Mount { updated: true }',
    );
    // The non-active file (bracket.ri) must not have been sent
    expect(vi.mocked(bridge.updateSource)).not.toHaveBeenCalledWith(
      '/project/bracket.ri',
      expect.anything(),
    );
  });
});

describe('App F5 re-evaluate uses live buffer content', () => {
  it('F5 sends the live buffer content (from liveContentRef getter), not the stale store snapshot', async () => {
    // Arrange: one file loaded
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      files: [{ path: '/project/bracket.ri', content: 'INITIAL' }],
    })));
    vi.mocked(bridge.updateSource).mockResolvedValue(undefined as any);

    render(() => <App />);
    await waitFor(() => expect(screen.getByTestId('app-layout')).toBeTruthy());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    await waitFor(() => expect(capturedEditorLiveContentRef).toBeDefined());

    // Store snapshot stays 'INITIAL' — simulate that no updateFileContent was called
    // (the store file content is still what it was at open time)
    const storeFile = capturedEditorStore!.state.openFiles.find(
      (f: any) => f.path === '/project/bracket.ri',
    );
    expect(storeFile?.content).toBe('INITIAL');

    // Wire the live content getter to return 'LIVE-EVAL', simulating a user
    // having typed into the editor (view doc diverged from store snapshot)
    capturedEditorLiveContentRef!(() => 'LIVE-EVAL');

    vi.mocked(bridge.updateSource).mockClear();

    // Act: press F5 to trigger handleReEvaluate
    fireEvent.keyDown(document, { key: 'F5' });

    // Assert: updateSource must be called with the LIVE content, not 'INITIAL'
    await waitFor(() => {
      expect(vi.mocked(bridge.updateSource)).toHaveBeenCalledTimes(1);
    });
    expect(vi.mocked(bridge.updateSource)).toHaveBeenCalledWith(
      '/project/bracket.ri',
      'LIVE-EVAL',
    );
  });
});

describe('App reload error toast', () => {
  it('shows error toast when reload fails', async () => {
    await withSuppressedRejectionsAndErrorSpy(async (errorSpy) => {
      // Set up a file-changed callback we can trigger
      let fileChangedCb!: (data: any) => void;
      vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
        fileChangedCb = cb;
        return () => {};
      });

      // Make bridgeOpenFile reject
      vi.mocked(bridge.openFile).mockRejectedValue(new Error('file not found'));

      vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
        files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
      })));

      render(() => <App />);

      // Wait for ready state and file-changed subscription
      await waitFor(() => {
        expect(screen.getByTestId('app-layout')).toBeTruthy();
        expect(fileChangedCb).toBeDefined();
      });
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      // Trigger file changed event to show the reload prompt
      capturedEditorStore.markDirty('/project/bracket.ri');
      fileChangedCb({ path: '/project/bracket.ri', content: 'updated' });

      await waitFor(() => {
        expect(screen.getByTestId('reload-prompt')).toBeTruthy();
      });

      capturedEditorStore.markClean('/project/bracket.ri');

      // Click the Reload button
      fireEvent.click(screen.getByText('Reload'));

      // Wait for the error toast to appear
      await waitFor(() => {
        const toastEl = screen.getByTestId('toast');
        expect(toastEl).toBeTruthy();
        expect(toastEl.dataset.type).toBe('error');
        expect(toastEl.textContent).toContain('failed to reload');
      });

      // console.error should NOT be called (replaced with toast)
      expect(errorSpy).not.toHaveBeenCalledWith(
        'Reload failed:',
        expect.any(Error),
      );
    });
  });
});

describe('App handleSave dirty-indicator and error handling', () => {
  it('clears dirty indicator after successful save via Ctrl+S', async () => {
    const path = '/project/test.ri';
    const content = 'module Test {}';

    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);

    render(() => <App />);

    // Wait for App to be ready and capturedEditorStore to be set
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Open a file in the store, mark it dirty, and set as active
    capturedEditorStore.openFile({ path, content });
    capturedEditorStore.markDirty(path);
    capturedEditorStore.setActiveFile(path);

    // Confirm the file is dirty before save
    expect(capturedEditorStore.state.dirtyFiles).toContain(path);

    // Trigger handleSave via global Ctrl+S shortcut
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    // bridge.saveFile should be called with the correct path and content
    await waitFor(() => {
      expect(bridge.saveFile).toHaveBeenCalledWith(path, content);
    });

    // After a successful save the dirty indicator must be cleared
    await waitFor(() => {
      expect(capturedEditorStore.state.dirtyFiles).not.toContain(path);
    });
  });

  it("shows 'Save failed' toast and keeps dirty indicator when bridge.saveFile rejects", async () => {
    await withSuppressedRejectionsAndErrorSpy(async (errorSpy) => {
      const path = '/project/test.ri';
      const content = 'module Test {}';

      vi.mocked(bridge.saveFile).mockRejectedValueOnce(new Error('disk full'));

      render(() => <App />);

      // Wait for App to be ready and capturedEditorStore to be set
      await waitFor(() => {
        expect(screen.getByTestId('app-layout')).toBeTruthy();
      });
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      // Open a file in the store, mark it dirty, and set as active
      capturedEditorStore.openFile({ path, content });
      capturedEditorStore.markDirty(path);
      capturedEditorStore.setActiveFile(path);

      // Confirm the file is dirty before the failing save
      expect(capturedEditorStore.state.dirtyFiles).toContain(path);

      // Trigger handleSave via global Ctrl+S shortcut
      fireEvent.keyDown(document, { key: 's', ctrlKey: true });

      // bridge.saveFile should be called with the correct path and content
      await waitFor(() => {
        expect(bridge.saveFile).toHaveBeenCalledWith(path, content);
      });

      // A toast containing "Save failed" and the error message must appear
      await waitFor(() => {
        const toasts = screen.getAllByTestId('toast');
        const errorToast = toasts.find(
          (t) => t.textContent?.includes('Save failed') && t.textContent?.includes('disk full'),
        );
        expect(errorToast).toBeTruthy();
      });

      // The dirty indicator must NOT be cleared (markClean was NOT called)
      expect(capturedEditorStore.state.dirtyFiles).toContain(path);

      // Targeted regression guard: handleSave must not regress into logging its failure
      // to console.error (the error must flow only through showToast). We do NOT use
      // `not.toHaveBeenCalled()` here because unrelated code paths (e.g. subscription
      // lifecycle, third-party libs) may legitimately emit console.error during this
      // test — this assertion tolerates that incidental noise while still flagging any
      // reintroduction of a `console.error('Save failed:', err)` call. Matches the
      // sibling pattern at setParameter (line ~1356) and re-evaluate (line ~1396).
      expect(errorSpy).not.toHaveBeenCalledWith('Save failed:', expect.any(Error));
    });
  });
});

describe('App handleSave uses live buffer content', () => {
  it('Ctrl+S sends the live buffer content (from liveContentRef getter), not the stale store snapshot', async () => {
    const path = '/project/test.ri';
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      files: [{ path, content: 'STALE' }],
    })));

    render(() => <App />);
    await waitFor(() => expect(screen.getByTestId('app-layout')).toBeTruthy());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    await waitFor(() => expect(capturedEditorLiveContentRef).toBeDefined());

    // Set store content to 'STALE' and mark file not externally-changed
    // so canSave returns ok — only the content argument changes
    capturedEditorStore!.setActiveFile(path);
    capturedEditorStore!.markDirty(path);
    // Store snapshot stays 'STALE' — no updateFileContent called after open

    // Wire the live content getter to return 'LIVE-SAVE'
    capturedEditorLiveContentRef!(() => 'LIVE-SAVE');

    vi.mocked(bridge.saveFile).mockClear();

    // Trigger handleSave via global Ctrl+S shortcut
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    // Assert live content was saved, NOT the stale store snapshot
    await waitFor(() => {
      expect(vi.mocked(bridge.saveFile)).toHaveBeenCalledTimes(1);
    });
    expect(vi.mocked(bridge.saveFile)).toHaveBeenCalledWith(path, 'LIVE-SAVE');
  });
});
