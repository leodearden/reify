import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@solidjs/testing-library';
import type { FileData, ModeState, PersistentViewState, PublishedState } from '../types';
import { flushMacrotasks, deferred, makeNode } from './test-utils';
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
import * as sidecarPersistence from '../stores/sidecarPersistence';
import { initDebugBridge as mockedInitDebugBridge } from '../debug';
import { primeAppHarness, renderAndWaitForReady, oneFileState } from './appHarness/harness';
import {
  capturedDualViewportProps,
  capturedEditorStore,
  capturedEditorOnOpen,
} from './appHarness/renderStubs';

beforeEach(primeAppHarness);

describe('App launcher Start view (alpha-spine β)', () => {
  beforeEach(() => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({ files: [] })));
  });

  function typeStartPath(path: string) {
    fireEvent.input(screen.getByTestId('start-path-input'), { target: { value: path } });
  }

  it('shows the start view over a still-mounted layout when no document is open', async () => {
    await renderAndWaitForReady();

    expect(screen.getByTestId('start-view')).toBeTruthy();
    expect(screen.getByTestId('app-layout')).toBeTruthy();
  });

  it('shows no start view when the launch opened a document', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(oneFileState()));

    await renderAndWaitForReady();

    expect(screen.queryByTestId('start-view')).toBeNull();
  });

  it('a launch file that failed to load shows the start view with E_GUI_OPEN_FAILED naming the file', async () => {
    vi.mocked(bridge.takeInitialFileLoadFailure).mockResolvedValue({
      path: '/designs/missing.ri',
      error: 'permission denied',
    });

    await renderAndWaitForReady();

    const error = screen.getByTestId('start-document-error');
    expect(error.textContent).toContain('E_GUI_OPEN_FAILED');
    expect(error.textContent).toContain('/designs/missing.ri');
    expect(screen.getByTestId('start-view')).toBeTruthy();
    expect(bridge.takeInitialFileLoadFailure).toHaveBeenCalledTimes(1);
    expect(vi.mocked(bridge.getInitialState).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(bridge.takeInitialFileLoadFailure).mock.invocationCallOrder[0],
    );
  });

  it('a launch that recorded a failure but still opened its document leaves no start-view error behind', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(
      published(makeGuiState({ files: [{ path: 'bolt.ri', content: 'structure Bolt {}' }] })),
    );
    vi.mocked(bridge.takeInitialFileLoadFailure).mockResolvedValue({
      path: '/designs/bolt.ri',
      error: 'engine panicked during check',
    });

    await renderAndWaitForReady();
    expect(screen.queryByTestId('start-view')).toBeNull();

    capturedEditorStore.closeFile('bolt.ri');

    await waitFor(() => expect(screen.getByTestId('start-view')).toBeTruthy());
    expect(screen.queryByTestId('start-document-error')).toBeNull();
  });

  it('a launch with no failure shows no start-view error', async () => {
    await renderAndWaitForReady();

    expect(screen.getByTestId('start-view')).toBeTruthy();
    expect(screen.queryByTestId('start-document-error')).toBeNull();
    expect(bridge.takeInitialFileLoadFailure).toHaveBeenCalledTimes(1);
  });

  it('an unreadable launch-file failure still reaches ready, toasts, and leaves no start-view error', async () => {
    vi.mocked(bridge.takeInitialFileLoadFailure).mockRejectedValue('ipc down');

    await renderAndWaitForReady();

    expect(screen.getByTestId('app-layout')).toBeTruthy();
    await waitFor(() =>
      expect(screen.queryAllByTestId('toast').some((t) => t.textContent?.includes('ipc down'))).toBe(true),
    );
    expect(screen.queryByTestId('start-document-error')).toBeNull();
  });

  it('opening a typed path loads it, records it as recent and dismisses the start view', async () => {
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/designs/bracket.ri', content: 'structure Bracket {}' });
    await renderAndWaitForReady();

    typeStartPath('/designs/bracket.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));

    await waitFor(() => expect(screen.queryByTestId('start-view')).toBeNull());
    expect(bridge.openFile).toHaveBeenCalledWith('/designs/bracket.ri');
    expect(bridge.openFileEngine).toHaveBeenCalledWith('/designs/bracket.ri');
    expect(vi.mocked(bridge.openFile).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(bridge.openFileEngine).mock.invocationCallOrder[0],
    );
    await waitFor(() => expect(bridge.recordRecent).toHaveBeenCalledWith('/designs/bracket.ri'));
  });

  it('a typed path opens under the canonical key open_file returned, not the typed spelling', async () => {
    const canonical = '/home/user/designs/bracket.ri';
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode({ entity_path: 'Root.A' })]);
    vi.mocked(bridge.openFile).mockResolvedValue({ path: canonical, content: 'structure Bracket {}' });
    await renderAndWaitForReady();

    typeStartPath('designs/../designs/bracket.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));

    await waitFor(() => expect(sidecarPersistence.loadSidecar).toHaveBeenCalledWith(canonical));
    expect(bridge.openFile).toHaveBeenCalledWith('designs/../designs/bracket.ri');
    expect(bridge.openFileEngine).toHaveBeenCalledWith(canonical);
    expect(capturedEditorStore.state.activeFile).toBe(canonical);

    fireEvent.click(await screen.findByRole('button', { name: 'Default' }));
    fireEvent.click(await screen.findByRole('menuitem', { name: /save views/i }));
    await waitFor(() =>
      expect(sidecarPersistence.saveSidecar).toHaveBeenCalledWith(canonical, expect.anything()),
    );
  });

  it('a failed open keeps the start view with E_GUI_OPEN_FAILED (B4)', async () => {
    vi.mocked(bridge.openFile).mockRejectedValue('No such file: /designs/missing.ri');
    await renderAndWaitForReady();

    typeStartPath('/designs/missing.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));

    const error = await screen.findByTestId('start-document-error');
    expect(error.textContent).toContain('E_GUI_OPEN_FAILED');
    expect(screen.getByTestId('start-view')).toBeTruthy();
    expect(bridge.recordRecent).not.toHaveBeenCalled();
  });

  function toastTexts(): string[] {
    return screen.queryAllByTestId('toast').map((t) => t.textContent ?? '');
  }

  function openPaths(): string[] {
    return capturedEditorStore.state.openFiles.map((f: { path: string }) => f.path);
  }

  it('an open the engine fails keeps the start view with E_GUI_OPEN_FAILED', async () => {
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/designs/broken.ri', content: 'structure {' });
    vi.mocked(bridge.openFileEngine).mockRejectedValue('engine panicked during check');
    await renderAndWaitForReady();

    typeStartPath('/designs/broken.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));

    const error = await screen.findByTestId('start-document-error');
    expect(error.textContent).toContain('E_GUI_OPEN_FAILED');
    expect(error.textContent).toContain('engine panicked during check');
    expect(screen.getByTestId('start-view')).toBeTruthy();
    expect(openPaths()).toEqual([]);
    expect(bridge.recordRecent).not.toHaveBeenCalled();
  });

  it('an open whose source the engine rejects opens it for repair with its compile diagnostics', async () => {
    const source = 'structure {';
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/designs/broken.ri', content: source });
    vi.mocked(bridge.openFileEngine).mockResolvedValue(
      published(
        makeGuiState({
          files: [{ path: '/designs/broken.ri', content: source }],
          compile_diagnostics: [
            {
              file_path: 'broken.ri',
              line: 1,
              column: 12,
              end_line: 1,
              end_column: 12,
              severity: 'Error',
              message: 'syntax error',
              code: null,
              has_location: true,
            },
          ],
        }),
      ),
    );
    await renderAndWaitForReady();

    typeStartPath('/designs/broken.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));

    await waitFor(() => expect(screen.queryByTestId('start-view')).toBeNull());
    expect(openPaths()).toEqual(['/designs/broken.ri']);
    await waitFor(() => expect(screen.getByTestId('diagnostics-count').textContent).toContain('1 error'));
    await waitFor(() => expect(bridge.recordRecent).toHaveBeenCalledWith('/designs/broken.ri'));
    expect(screen.queryByTestId('start-document-error')).toBeNull();
  });

  it('an unreadable view sidecar still opens the document and says saved views were not restored', async () => {
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/designs/bracket.ri', content: 'structure Bracket {}' });
    vi.mocked(sidecarPersistence.loadSidecar).mockRejectedValue(
      'Error parsing /designs/bracket.ri.views.json: expected value',
    );
    await renderAndWaitForReady();

    typeStartPath('/designs/bracket.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));

    await waitFor(() => expect(screen.queryByTestId('start-view')).toBeNull());
    await waitFor(() =>
      expect(
        toastTexts().some((t) => t.includes('Saved views not restored') && t.includes('Error parsing')),
      ).toBe(true),
    );
    await waitFor(() => expect(bridge.recordRecent).toHaveBeenCalledWith('/designs/bracket.ri'));
  });

  it('a File→Open the engine fails leaves the open document untouched and toasts the code', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(oneFileState()));
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/project/broken.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/project/broken.ri', content: 'structure {' });
    vi.mocked(bridge.openFileEngine).mockRejectedValue('engine panicked during check');
    await renderAndWaitForReady();

    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });

    await waitFor(() =>
      expect(toastTexts().some((t) => t.includes('Open file failed: E_GUI_OPEN_FAILED'))).toBe(true),
    );
    expect(openPaths()).toEqual(['/project/bracket.ri']);
    expect(capturedEditorStore.state.activeFile).toBe('/project/bracket.ri');
  });

  it('a File→Open that dismisses the start view leaves no stale document error behind', async () => {
    vi.mocked(bridge.openFile)
      .mockRejectedValueOnce('No such file: /designs/missing.ri')
      .mockResolvedValueOnce({ path: '/designs/bracket.ri', content: 'structure Bracket {}' });
    await renderAndWaitForReady();

    typeStartPath('/designs/missing.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));
    await screen.findByTestId('start-document-error');

    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/designs/bracket.ri');
    await vi.waitFor(() => expect(capturedEditorOnOpen).toBeDefined());
    capturedEditorOnOpen!();
    await waitFor(() => expect(screen.queryByTestId('start-view')).toBeNull());

    capturedEditorStore.closeFile('/designs/bracket.ri');

    await screen.findByTestId('start-view');
    expect(screen.queryByTestId('start-document-error')).toBeNull();
  });

  it('creating a typed path writes the template, then opens the path the backend returned', async () => {
    vi.mocked(bridge.createNewFile).mockResolvedValue({ path: '/designs/created.ri', content: '// New design\n' });
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/designs/created.ri', content: '// New design\n' });
    await renderAndWaitForReady();

    typeStartPath('/designs/created');
    fireEvent.click(screen.getByTestId('start-create-path'));

    await waitFor(() => expect(bridge.openFileEngine).toHaveBeenCalledWith('/designs/created.ri'));
    expect(bridge.createNewFile).toHaveBeenCalledWith('/designs/created');
    expect(bridge.openFile).toHaveBeenCalledWith('/designs/created.ri');
    expect(bridge.saveFile).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByTestId('start-view')).toBeNull());
  });

  it('a Start-view open the engine is still loading shows Opening <path>… with the document actions disabled until it loads', async () => {
    vi.mocked(bridge.listRecent).mockResolvedValue([{ path: '/designs/flange.ri', opened_at: 1000 }]);
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/designs/bracket.ri', content: 'structure Bracket {}' });
    const reply = deferred<PublishedState>();
    vi.mocked(bridge.openFileEngine).mockReturnValue(reply.promise);
    await renderAndWaitForReady();
    await screen.findByTestId('start-recent-item');

    typeStartPath('/designs/bracket.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));

    const pendingLine = await screen.findByTestId('start-document-pending');
    expect(within(pendingLine).getByRole('status').textContent).toBe('Opening /designs/bracket.ri…');
    for (const id of ['start-open-path', 'start-open-dialog', 'start-new-dialog', 'start-create-path', 'start-recent-item']) {
      expect((screen.getByTestId(id) as HTMLButtonElement).disabled).toBe(true);
    }

    reply.resolve(published(makeGuiState()));
    await waitFor(() => expect(screen.queryByTestId('start-view')).toBeNull());
  });

  it('a Ctrl+O while a Start-view open is still loading toasts E_GUI_DOCUMENT_BUSY before opening the file dialog and starts no second load', async () => {
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/designs/bracket.ri', content: 'structure Bracket {}' });
    const reply = deferred<PublishedState>();
    vi.mocked(bridge.openFileEngine).mockReturnValue(reply.promise);
    await renderAndWaitForReady();

    typeStartPath('/designs/bracket.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));
    await waitFor(() => expect(bridge.openFileEngine).toHaveBeenCalledWith('/designs/bracket.ri'));

    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/designs/flange.ri');
    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });

    await waitFor(() =>
      expect(
        toastTexts().some((t) => t.includes('Open file failed: E_GUI_DOCUMENT_BUSY') && t.includes('/designs/bracket.ri is still opening')),
      ).toBe(true),
    );
    expect(bridge.pickOpenPath).not.toHaveBeenCalled();
    expect(bridge.openFile).not.toHaveBeenCalledWith('/designs/flange.ri');
    expect(bridge.openFileEngine).toHaveBeenCalledTimes(1);

    reply.resolve(published(makeGuiState()));
    await waitFor(() => expect(screen.queryByTestId('start-view')).toBeNull());
    expect(openPaths()).toEqual(['/designs/bracket.ri']);
  });

  it('a Ctrl+N while a Start-view open is still loading toasts E_GUI_DOCUMENT_BUSY before opening the save dialog', async () => {
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/designs/bracket.ri', content: 'structure Bracket {}' });
    const reply = deferred<PublishedState>();
    vi.mocked(bridge.openFileEngine).mockReturnValue(reply.promise);
    await renderAndWaitForReady();

    typeStartPath('/designs/bracket.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));
    await waitFor(() => expect(bridge.openFileEngine).toHaveBeenCalledWith('/designs/bracket.ri'));

    vi.mocked(bridge.pickSavePath).mockResolvedValue('/designs/new.ri');
    fireEvent.keyDown(document, { key: 'n', ctrlKey: true });

    await waitFor(() =>
      expect(
        toastTexts().some(
          (t) => t.includes('New file failed: E_GUI_DOCUMENT_BUSY') && t.includes('/designs/bracket.ri is still opening'),
        ),
      ).toBe(true),
    );
    expect(bridge.pickSavePath).not.toHaveBeenCalled();
    expect(bridge.createNewFile).not.toHaveBeenCalled();
    expect(bridge.openFileEngine).toHaveBeenCalledTimes(1);

    reply.resolve(published(makeGuiState()));
    await waitFor(() => expect(screen.queryByTestId('start-view')).toBeNull());
  });

  it.each([
    { shortcut: 'Ctrl+O', key: 'o', refusal: 'Open file failed: E_GUI_DOCUMENT_BUSY', picker: 'pickOpenPath' },
    { shortcut: 'Ctrl+N', key: 'n', refusal: 'New file failed: E_GUI_DOCUMENT_BUSY', picker: 'pickSavePath' },
  ] as const)('a $shortcut with unsaved changes while a Start-view open is still loading refuses without the discard prompt', async ({ key, refusal, picker }) => {
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/designs/bracket.ri', content: 'structure Bracket {}' });
    const reply = deferred<PublishedState>();
    vi.mocked(bridge.openFileEngine).mockReturnValue(reply.promise);
    await renderAndWaitForReady();

    typeStartPath('/designs/bracket.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));
    await waitFor(() => expect(bridge.openFileEngine).toHaveBeenCalledWith('/designs/bracket.ri'));

    capturedEditorStore.markDirty('/project/bracket.ri');
    vi.mocked((bridge as any).ask).mockResolvedValue(true);
    vi.mocked(bridge[picker]).mockResolvedValue('/designs/flange.ri');
    fireEvent.keyDown(document, { key, ctrlKey: true });

    await waitFor(() =>
      expect(
        toastTexts().some((t) => t.includes(refusal) && t.includes('/designs/bracket.ri is still opening')),
      ).toBe(true),
    );
    expect(bridge.ask).not.toHaveBeenCalled();
    expect(bridge[picker]).not.toHaveBeenCalled();

    reply.resolve(published(makeGuiState()));
    await waitFor(() => expect(screen.queryByTestId('start-view')).toBeNull());
  });

  it('a Start-view open abandoned while the engine is loading never lands, even after another document opened', async () => {
    vi.mocked(bridge.openFile)
      .mockResolvedValueOnce({ path: '/designs/bracket.ri', content: 'structure Bracket {}' })
      .mockResolvedValueOnce({ path: '/designs/flange.ri', content: 'structure Flange {}' });
    const abandonedReply = deferred<PublishedState>();
    vi.mocked(bridge.openFileEngine)
      .mockReturnValueOnce(abandonedReply.promise)
      .mockResolvedValue(published(makeGuiState()));
    await renderAndWaitForReady();

    typeStartPath('/designs/bracket.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));
    await screen.findByTestId('start-document-pending');
    await waitFor(() => expect(bridge.openFileEngine).toHaveBeenCalledWith('/designs/bracket.ri'));
    fireEvent.click(screen.getByTestId('start-document-cancel'));
    await waitFor(() => expect(screen.queryByTestId('start-document-pending')).toBeNull());

    typeStartPath('/designs/flange.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));
    await waitFor(() => expect(screen.queryByTestId('start-view')).toBeNull());
    expect(openPaths()).toEqual(['/designs/flange.ri']);

    abandonedReply.resolve(published(makeGuiState()));
    await flushMacrotasks();

    expect(openPaths()).toEqual(['/designs/flange.ri']);
    expect(capturedEditorStore.state.activeFile).toBe('/designs/flange.ri');
  });

  it('a Start-view open abandoned while reading the file sends no engine request', async () => {
    const read = deferred<FileData>();
    vi.mocked(bridge.openFile).mockReturnValueOnce(read.promise);
    await renderAndWaitForReady();

    typeStartPath('/designs/bracket.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));
    await screen.findByTestId('start-document-pending');
    fireEvent.click(screen.getByTestId('start-document-cancel'));

    read.resolve({ path: '/designs/bracket.ri', content: 'structure Bracket {}' });
    await flushMacrotasks();

    expect(bridge.openFileEngine).not.toHaveBeenCalled();
    expect(screen.getByTestId('start-view')).toBeTruthy();
    expect(openPaths()).toEqual([]);
  });

  it('an open abandoned after its document was closed restores no views over the document opened since', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode({ entity_path: 'Root.A' })]);
    vi.mocked(bridge.openFile)
      .mockResolvedValueOnce({ path: '/designs/bracket.ri', content: 'structure Bracket {}' })
      .mockResolvedValueOnce({ path: '/designs/flange.ri', content: 'structure Flange {}' });
    const heldViews = deferred<PersistentViewState | null>();
    vi.mocked(sidecarPersistence.loadSidecar).mockReturnValueOnce(heldViews.promise);
    await renderAndWaitForReady();

    typeStartPath('/designs/bracket.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));
    await waitFor(() => expect(screen.queryByTestId('start-view')).toBeNull());
    await waitFor(() => expect(sidecarPersistence.loadSidecar).toHaveBeenCalledWith('/designs/bracket.ri'));

    capturedEditorStore.closeFile('/designs/bracket.ri');
    await screen.findByTestId('start-document-pending');
    fireEvent.click(screen.getByTestId('start-document-cancel'));
    await waitFor(() => expect(screen.queryByTestId('start-document-pending')).toBeNull());

    typeStartPath('/designs/flange.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));
    await waitFor(() => expect(screen.queryByTestId('start-view')).toBeNull());
    await waitFor(() => expect(bridge.recordRecent).toHaveBeenCalledWith('/designs/flange.ri'));

    heldViews.resolve(null);
    await flushMacrotasks();

    fireEvent.click(await screen.findByRole('button', { name: 'Default' }));
    fireEvent.click(await screen.findByRole('menuitem', { name: /save views/i }));
    await waitFor(() => expect(sidecarPersistence.saveSidecar).toHaveBeenCalled());
    expect(sidecarPersistence.saveSidecar).toHaveBeenCalledWith('/designs/flange.ri', expect.anything());
    expect(sidecarPersistence.saveSidecar).not.toHaveBeenCalledWith('/designs/bracket.ri', expect.anything());
    expect(capturedEditorStore.state.activeFile).toBe('/designs/flange.ri');
  });

  it('Design closes the chat panel and Assistant reopens it', async () => {
    vi.mocked(bridge.setMode).mockImplementation(async (mode) => ({ mode, debug_port: null }));
    await renderAndWaitForReady();
    expect(screen.getByTestId('chat-panel')).toBeTruthy();

    fireEvent.click(screen.getByTestId('start-mode-design'));
    await waitFor(() => expect(screen.queryByTestId('chat-panel')).toBeNull());
    expect(bridge.setMode).toHaveBeenCalledWith('design');
    expect(screen.getByTestId('start-mode-design').getAttribute('aria-pressed')).toBe('true');

    fireEvent.click(screen.getByTestId('start-mode-assistant'));
    await waitFor(() => expect(screen.getByTestId('chat-panel')).toBeTruthy());
    expect(bridge.setMode).toHaveBeenCalledWith('assistant');
  });

  it('Debug selects debug mode through set_mode and starts the debug bridge', async () => {
    vi.mocked(bridge.setMode).mockImplementation(async (mode) => ({
      mode,
      debug_port: mode === 'debug' ? 3939 : null,
    }));
    await renderAndWaitForReady();

    fireEvent.click(screen.getByTestId('start-mode-debug'));

    await waitFor(() => expect(mockedInitDebugBridge).toHaveBeenCalledTimes(1));
    expect(bridge.setMode).toHaveBeenCalledWith('debug');
    expect(screen.getByTestId('start-mode-debug').getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByTestId('start-debug-port').textContent).toContain('3939');
  });

  it('a second Debug click sent before the first set_mode reply starts no second debug bridge', async () => {
    const firstReply = deferred<ModeState>();
    const secondReply = deferred<ModeState>();
    vi.mocked(bridge.setMode).mockReturnValueOnce(firstReply.promise).mockReturnValueOnce(secondReply.promise);
    await renderAndWaitForReady();

    fireEvent.click(screen.getByTestId('start-mode-debug'));
    fireEvent.click(screen.getByTestId('start-mode-debug'));
    await waitFor(() => expect(bridge.setMode).toHaveBeenCalledTimes(2));

    firstReply.resolve({ mode: 'debug', debug_port: 3939 });
    await waitFor(() => expect(mockedInitDebugBridge).toHaveBeenCalledTimes(1));
    secondReply.resolve({ mode: 'debug', debug_port: 3939 });
    await flushMacrotasks(50);

    expect(mockedInitDebugBridge).toHaveBeenCalledTimes(1);
  });

  it('a debug bridge that fails to start shows E_GUI_DEBUG_BRIDGE_FAILED, and Debug again retries it', async () => {
    vi.mocked(bridge.setMode).mockImplementation(async (mode) => ({
      mode,
      debug_port: mode === 'debug' ? 3939 : null,
    }));
    vi.mocked(mockedInitDebugBridge).mockRejectedValueOnce(new Error('debug-request subscription failed'));
    await renderAndWaitForReady();

    fireEvent.click(screen.getByTestId('start-mode-debug'));

    const error = await screen.findByTestId('start-mode-error');
    expect(error.textContent).toContain('E_GUI_DEBUG_BRIDGE_FAILED');
    expect(error.textContent).not.toContain('E_GUI_DEBUG_LISTENER_FAILED');
    expect(error.textContent).toContain('debug-request subscription failed');

    fireEvent.click(screen.getByTestId('start-mode-debug'));

    await waitFor(() => expect(mockedInitDebugBridge).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByTestId('start-mode-error')).toBeNull());
  });

  it('a repeat Debug click whose set_mode fails shows E_GUI_MODE_FAILED and keeps Debug selected', async () => {
    vi.mocked(bridge.setMode)
      .mockResolvedValueOnce({ mode: 'debug', debug_port: 3939 })
      .mockRejectedValueOnce('IPC channel closed');
    await renderAndWaitForReady();

    fireEvent.click(screen.getByTestId('start-mode-debug'));
    await waitFor(() => expect(mockedInitDebugBridge).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(screen.getByTestId('start-mode-debug').getAttribute('aria-pressed')).toBe('true'),
    );

    fireEvent.click(screen.getByTestId('start-mode-debug'));

    const error = await screen.findByTestId('start-mode-error');
    expect(error.textContent).toContain('E_GUI_MODE_FAILED');
    expect(error.textContent).toContain('IPC channel closed');
    expect(error.textContent).not.toContain('E_GUI_DEBUG_LISTENER_FAILED');
    expect(screen.getByTestId('start-mode-debug').getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByTestId('start-debug-port').textContent).toContain('3939');
    expect(mockedInitDebugBridge).toHaveBeenCalledTimes(1);
  });

  it('a Debug click after an env-launched debug session whose set_mode fails shows E_GUI_MODE_FAILED', async () => {
    vi.mocked(bridge.isDebugEnabled).mockResolvedValue(true);
    vi.mocked(bridge.setMode)
      .mockResolvedValueOnce({ mode: 'debug', debug_port: 3939 })
      .mockRejectedValueOnce('IPC channel closed');
    await renderAndWaitForReady();
    await waitFor(() =>
      expect(screen.getByTestId('start-mode-debug').getAttribute('aria-pressed')).toBe('true'),
    );

    fireEvent.click(screen.getByTestId('start-mode-debug'));

    const error = await screen.findByTestId('start-mode-error');
    expect(error.textContent).toContain('E_GUI_MODE_FAILED');
    expect(error.textContent).toContain('IPC channel closed');
    expect(error.textContent).not.toContain('E_GUI_DEBUG_LISTENER_FAILED');
  });

  it('a Design click whose set_mode fails shows E_GUI_MODE_FAILED and leaves the chat panel open', async () => {
    vi.mocked(bridge.setMode).mockImplementation((mode) =>
      mode === 'design'
        ? Promise.reject('state not managed')
        : Promise.resolve({ mode, debug_port: mode === 'debug' ? 3939 : null }),
    );
    await renderAndWaitForReady();

    fireEvent.click(screen.getByTestId('start-mode-design'));

    const error = await screen.findByTestId('start-mode-error');
    expect(error.textContent).toContain('E_GUI_MODE_FAILED');
    expect(error.textContent).toContain('state not managed');
    expect(error.textContent).not.toContain('E_GUI_DEBUG_LISTENER_FAILED');
    expect(screen.getByTestId('chat-panel')).toBeTruthy();
    expect(mockedInitDebugBridge).not.toHaveBeenCalled();
  });

  it('an env-launched debug session reports Debug mode', async () => {
    vi.mocked(bridge.isDebugEnabled).mockResolvedValue(true);
    vi.mocked(bridge.setMode).mockImplementation(async (mode) => ({
      mode,
      debug_port: mode === 'debug' ? 3939 : null,
    }));

    await renderAndWaitForReady();

    expect(bridge.setMode).toHaveBeenCalledWith('debug');
    expect(screen.getByTestId('start-mode-debug').getAttribute('aria-pressed')).toBe('true');
    expect(mockedInitDebugBridge).toHaveBeenCalledTimes(1);
  });

  it('an env-launched debug session starts the debug bridge before set_mode replies', async () => {
    vi.mocked(bridge.isDebugEnabled).mockResolvedValue(true);
    const reply = deferred<ModeState>();
    vi.mocked(bridge.setMode).mockReturnValue(reply.promise);

    render(() => <App />);

    await waitFor(() => expect(mockedInitDebugBridge).toHaveBeenCalledTimes(1));

    reply.resolve({ mode: 'debug', debug_port: 3939 });
    await waitFor(() => expect(screen.getByTestId('app-layout')).toBeTruthy());
    await waitFor(() =>
      expect(screen.getByTestId('start-mode-debug').getAttribute('aria-pressed')).toBe('true'),
    );
  });

  it('an env-launched debug session whose set_mode fails still starts the debug bridge and shows the set_mode failure', async () => {
    vi.mocked(bridge.isDebugEnabled).mockResolvedValue(true);
    vi.mocked(bridge.setMode).mockRejectedValue('IPC channel closed');

    await renderAndWaitForReady();

    expect(mockedInitDebugBridge).toHaveBeenCalledTimes(1);
    const error = await screen.findByTestId('start-mode-error');
    expect(error.textContent).toContain('E_GUI_MODE_FAILED');
    expect(error.textContent).not.toContain('E_GUI_DEBUG_LISTENER_FAILED');
    expect(error.textContent).toContain('IPC channel closed');
    expect(screen.getByTestId('start-mode-debug').getAttribute('aria-pressed')).not.toBe('true');
    const toast = await screen.findByTestId('toast');
    expect(toast.textContent).toContain('set_mode');
    expect(toast.textContent).toContain('IPC channel closed');
  });

  it('an env-launched debug session whose bridge fails shows the failure as the mode error and toasts it', async () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      vi.mocked(bridge.isDebugEnabled).mockResolvedValue(true);
      vi.mocked(bridge.setMode).mockImplementation(async (mode) => ({
        mode,
        debug_port: mode === 'debug' ? 3939 : null,
      }));
      vi.mocked(mockedInitDebugBridge).mockRejectedValueOnce(new Error('debug-request subscription failed'));

      await renderAndWaitForReady();

      const error = await screen.findByTestId('start-mode-error');
      expect(error.textContent).toContain('E_GUI_DEBUG_BRIDGE_FAILED');
      expect(error.textContent).toContain('debug-request subscription failed');
      const toast = await screen.findByTestId('toast');
      expect(toast.dataset.type).toBe('error');
      expect(toast.textContent).toContain('debug-request subscription failed');
      expect(errorSpy).toHaveBeenCalledWith('[debug-bridge] init failed:', expect.anything());
    } finally {
      errorSpy.mockRestore();
    }
  });

  it('an env-launched debug session whose bridge and set_mode both fail shows the bridge failure as the mode error', async () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      vi.mocked(bridge.isDebugEnabled).mockResolvedValue(true);
      vi.mocked(bridge.setMode).mockRejectedValue('IPC channel closed');
      vi.mocked(mockedInitDebugBridge).mockRejectedValueOnce(new Error('debug-request subscription failed'));

      await renderAndWaitForReady();

      await waitFor(() =>
        expect(screen.getByTestId('start-mode-error').textContent).toContain('debug-request subscription failed'),
      );
      expect(screen.getByTestId('start-mode-error').textContent).toContain('E_GUI_DEBUG_BRIDGE_FAILED');
      expect(screen.getByTestId('start-mode-error').textContent).not.toContain('IPC channel closed');
      const toasts = await screen.findAllByTestId('toast');
      expect(toasts.map((t) => t.textContent).join('\n')).toContain('IPC channel closed');
    } finally {
      errorSpy.mockRestore();
    }
  });

  it('the help and export shortcuts open nothing behind the start view', async () => {
    await renderAndWaitForReady();

    fireEvent.keyDown(document, { key: '?' });
    fireEvent.keyDown(document, { key: 'e', ctrlKey: true });

    expect(screen.queryByTestId('keyboard-help')).toBeNull();
    expect(screen.queryByTestId('export-dialog')).toBeNull();
    expect(screen.getByTestId('start-view')).toBeTruthy();
  });

  it('layout shortcuts leave the covered layout untouched', async () => {
    await renderAndWaitForReady();

    fireEvent.keyDown(document, { key: 'e' });
    fireEvent.keyDown(document, { key: 'j', ctrlKey: true });
    fireEvent.keyDown(document, { key: 'm', ctrlKey: true, shiftKey: true });

    expect(screen.getByTestId('toolbar-edges-toggle').getAttribute('aria-pressed')).toBe('false');
    expect(capturedDualViewportProps.showEdges).toBe(false);
    expect(screen.getByTestId('chat-panel')).toBeTruthy();
    expect(screen.queryByTestId('splitter-problems')).toBeNull();
  });

  it('the help shortcut works once a document opens from the start view', async () => {
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/designs/bracket.ri', content: 'structure Bracket {}' });
    await renderAndWaitForReady();

    typeStartPath('/designs/bracket.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));
    await waitFor(() => expect(screen.queryByTestId('start-view')).toBeNull());

    fireEvent.keyDown(document, { key: '?' });
    await waitFor(() => expect(screen.getByTestId('keyboard-help')).toBeTruthy());
  });
});
