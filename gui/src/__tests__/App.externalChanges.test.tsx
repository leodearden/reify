import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@solidjs/testing-library';
import {
  EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG,
  SAVE_CONFLICT_RELOAD_LABEL,
  SAVE_CONFLICT_OVERWRITE_LABEL,
} from '../editor/messages';
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

// ─── externallyChanged wiring in App.tsx ─────────────────────────────────────

describe('App externallyChanged store wiring', () => {
  const testState = makeGuiState({
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
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

  it('(a) onFileChanged for an open file adds it to externallyChanged', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Must be dirty for the externally-changed path to trigger (non-dirty auto-reloads silently)
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'new' });

    await waitFor(() => {
      expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri');
    });
  });

  it('(b) onFileChanged for a path NOT in openFiles does NOT add to externallyChanged', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // '/project/other.ri' is not in the initial state files
    fileChangedCallback!({ path: '/project/other.ri', content: 'new' });

    // Give reactivity a chance to settle
    await new Promise((r) => setTimeout(r, 10));
    expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/other.ri');
  });

  it('(c) after handleReload succeeds, path is removed from externallyChanged', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Must be dirty so the event triggers the externally-changed path (not silent auto-reload)
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'new' });
    await waitFor(() =>
      expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri'),
    );

    // The file is dirty, so clicking Reload first shows the "Reload Anyway" confirmation.
    await waitFor(() => expect(screen.getByText('Reload')).toBeTruthy());
    fireEvent.click(screen.getByText('Reload'));

    // After the dirty-overlap confirmation appears, clicking "Reload Anyway" proceeds.
    await waitFor(() => expect(screen.getByText('Reload Anyway')).toBeTruthy());
    fireEvent.click(screen.getByText('Reload Anyway'));

    await waitFor(() => {
      expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');
    });
  });

  it('(c2) after handleReload succeeds, dirtyFiles is also cleared when the file had unsaved edits', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Simulate: user had typed unsaved edits AND an external file-changed event arrives
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'disk content' });
    await waitFor(() =>
      expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri'),
    );
    expect(capturedEditorStore.state.dirtyFiles).toContain('/project/bracket.ri');

    // First Reload click: dirty overlap detected → shows "Reload Anyway" confirmation
    await waitFor(() => expect(screen.getByText('Reload')).toBeTruthy());
    fireEvent.click(screen.getByText('Reload'));

    // Second click on "Reload Anyway" → proceeds with actual reload
    await waitFor(() => expect(screen.getByText('Reload Anyway')).toBeTruthy());
    fireEvent.click(screen.getByText('Reload Anyway'));

    // After reload both flags are cleared: the buffer was replaced with disk
    // content so neither user-edits nor disk-divergence remain.
    await waitFor(() => {
      expect(capturedEditorStore.state.dirtyFiles).not.toContain('/project/bracket.ri');
    });
    await waitFor(() => {
      expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');
    });
  });

  it('(d) handleDismissReload clears all paths from externallyChanged', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Must be dirty so the event triggers the externally-changed path (not silent auto-reload)
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'new' });
    await waitFor(() =>
      expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri'),
    );

    // Click Dismiss — independent of dirty state, clears all changedFiles + externallyChanged
    await waitFor(() => expect(screen.getByText('Dismiss')).toBeTruthy());
    fireEvent.click(screen.getByText('Dismiss'));

    await waitFor(() => {
      expect(capturedEditorStore.state.externallyChanged).toEqual([]);
    });
  });

  it('(e) handleDismissReload with multiple paths clears all entries from externallyChanged', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Must be dirty so the event triggers the externally-changed path (not silent auto-reload)
    capturedEditorStore.markDirty('/project/bracket.ri');
    // Trigger changedFiles for bracket.ri so the Dismiss button appears
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'new' });
    await waitFor(() =>
      expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri'),
    );

    // Add two more paths directly (simulates concurrent disk changes to other open files)
    capturedEditorStore.markExternallyChanged('/project/b.ri');
    capturedEditorStore.markExternallyChanged('/project/c.ri');
    expect(capturedEditorStore.state.externallyChanged.length).toBe(3);
    expect(capturedEditorStore.state.externallyChanged).toEqual(
      expect.arrayContaining(['/project/bracket.ri', '/project/b.ri', '/project/c.ri']),
    );

    // Click Dismiss — should atomically clear all three
    await waitFor(() => expect(screen.getByText('Dismiss')).toBeTruthy());
    fireEvent.click(screen.getByText('Dismiss'));

    await waitFor(() => {
      expect(capturedEditorStore.state.externallyChanged).toEqual([]);
    });
  });
});

// ─── Auto-reload non-dirty tabs on file-changed event ────────────────────────

describe('App file-changed auto-reload (non-dirty)', () => {
  const testState = makeGuiState({
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
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

  it('(a) file-changed for a non-dirty open tab silently updates content without setting externallyChanged', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // File is NOT dirty (no markDirty call)
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'auto-reloaded content' });

    // Give reactivity a chance to settle
    await new Promise((r) => setTimeout(r, 20));

    // Content should be updated in the store
    const file = capturedEditorStore.state.openFiles.find(
      (f: any) => f.path === '/project/bracket.ri',
    );
    expect(file?.content).toBe('auto-reloaded content');

    // externallyChanged should remain empty — no conflict to surface
    expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');

    // No reload-prompt banner should be visible (changedFiles is empty)
    expect(screen.queryByText('Reload')).toBeNull();

    // The auto-reload must NOT mark the file dirty — markClean runs after updateFileContent,
    // so dirtyFiles should not contain this path.
    expect(capturedEditorStore.state.dirtyFiles).not.toContain('/project/bracket.ri');

    // The auto-reload must NOT echo back to the backend as a phantom user edit.
    // (Editor is mocked in App tests, so no CodeMirror view, but this is a guard for
    // the future when App tests may use the real Editor.)
    expect(bridge.updateSource).not.toHaveBeenCalled();
  });

  it('(b) file-changed for a DIRTY open tab still triggers externallyChanged and shows the Reload banner', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Mark the file as dirty (user has unsaved edits)
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'disk changed' });

    // Dirty path: externallyChanged should be set and the banner rendered
    await waitFor(() => {
      expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri');
    });
    await waitFor(() => {
      expect(screen.getByText('Reload')).toBeTruthy();
    });
  });

  it('(c) file-changed for a path NOT in openFiles does nothing', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    fileChangedCallback!({ path: '/project/unknown.ri', content: 'some content' });

    await new Promise((r) => setTimeout(r, 20));

    // No state mutation, no banner, no toast
    expect(capturedEditorStore.state.externallyChanged).toEqual([]);
    expect(screen.queryByText('Reload')).toBeNull();
  });
});

// ─── isSameFile cross-format path matching in onFileChanged ─────────────────

describe('App file-changed isSameFile cross-format matching', () => {
  const testState = makeGuiState({
    files: [
      { path: '/project/foo.ri', content: 'old content' },
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

  it('file-changed with file:// URI matches a bare-path tab and auto-reloads its content', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Tab has bare path '/project/foo.ri'; event arrives with file:// URI.
    // The handler must use isSameFile to match them.
    fileChangedCallback!({ path: 'file:///project/foo.ri', content: 'NEW' });

    // Give reactivity a chance to settle
    await new Promise((r) => setTimeout(r, 20));

    // Content should be updated — the URI matched the bare-path tab
    const file = capturedEditorStore.state.openFiles.find(
      (f: any) => f.path === '/project/foo.ri',
    );
    expect(file?.content).toBe('NEW');

    // No conflict UI — non-dirty auto-reload
    expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/foo.ri');
    expect(screen.queryByText('Reload')).toBeNull();
  });
});

// ─── handleSave aborts when file is externally changed ───────────────────────

describe('App handleSave aborts when file is externally changed', () => {
  const testState = makeGuiState({
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
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

  it('(a) handleSave does NOT call bridgeSaveFile and shows conflict prompt when active file is externally changed', async () => {
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);

    // Regression guard: neither handleSave nor the Editor keymap should emit
    // console.error for the externally-changed branch (only for not-found).
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    try {
      render(() => <App />);
      await waitFor(() => expect(fileChangedCallback).toBeDefined());
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      // Set active file
      capturedEditorStore.setActiveFile('/project/bracket.ri');

      // Mark the file as externally changed
      capturedEditorStore.markExternallyChanged('/project/bracket.ri');

      // Trigger handleSave via Ctrl+S
      fireEvent.keyDown(document, { key: 's', ctrlKey: true });

      // Wait a beat for any async effects
      await new Promise((r) => setTimeout(r, 20));

      // bridgeSaveFile (bridge.saveFile) must NOT have been called
      expect(bridge.saveFile).not.toHaveBeenCalled();

      // Neither handleSave nor the Editor keymap should emit 'Save aborted' for
      // the externally-changed branch (only for not-found). We narrow the check
      // to the specific message substring so that unrelated SolidJS / dev-mode
      // console.error calls don't cause spurious failures.
      expect(
        consoleErrorSpy.mock.calls
          .flat()
          .some((arg: unknown) => typeof arg === 'string' && arg.includes('Save aborted')),
      ).toBe(false);

      // A conflict toast (not a dead-end error) must appear with the prompt message
      // and both action buttons. Using the full constant (not a loose keyword regex)
      // enforces lockstep: a wording drift in messages.ts would break this test.
      await waitFor(() => {
        const toasts = screen.getAllByTestId('toast');
        const conflictToast = toasts.find((t) =>
          t.textContent?.includes(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG),
        );
        expect(conflictToast).toBeTruthy();
        // Must be 'error' type (sticky, no auto-dismiss) — matches the old blocked toast
        expect(conflictToast?.dataset.type).toBe('error');
        // Both action buttons must be present within the toast
        expect(within(conflictToast!).getByRole('button', { name: SAVE_CONFLICT_RELOAD_LABEL })).toBeTruthy();
        expect(within(conflictToast!).getByRole('button', { name: SAVE_CONFLICT_OVERWRITE_LABEL })).toBeTruthy();
      });
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it('(b) after clearExternallyChanged, handleSave DOES call bridgeSaveFile', async () => {
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.setActiveFile('/project/bracket.ri');
    capturedEditorStore.markExternallyChanged('/project/bracket.ri');
    // Clear the external-change flag — save should now proceed
    capturedEditorStore.clearExternallyChanged('/project/bracket.ri');

    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    await waitFor(() => {
      expect(bridge.saveFile).toHaveBeenCalledWith(
        '/project/bracket.ri',
        expect.any(String),
      );
    });
  });

  it('(c) two Ctrl+S in a row while externally changed produce only one conflict prompt', async () => {
    // Guards against stacked prompts: if the user reflexively presses Ctrl+S twice,
    // showSaveConflictPrompt must deduplicate so only one toast is visible.  A stacked
    // second prompt could silently discard newer edits if the user clicks Reload in
    // the older copy after having typed more into the buffer.
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.setActiveFile('/project/bracket.ri');
    capturedEditorStore.markExternallyChanged('/project/bracket.ri');

    // Press Ctrl+S twice in quick succession
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    await new Promise((r) => setTimeout(r, 20));

    // Exactly ONE conflict toast must be visible — not two stacked copies
    const conflictToasts = screen
      .getAllByTestId('toast')
      .filter((t) => t.textContent?.includes(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG));
    expect(conflictToasts).toHaveLength(1);
  });
});

// ─── Conflict prompt: Reload from disk action ────────────────────────────────

describe('App handleSave conflict prompt: Reload from disk', () => {
  const testState = makeGuiState({
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
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

  it('clicking Reload from disk calls bridgeOpenFile, updates content, and clears both dirty/externallyChanged flags', async () => {
    const diskContent = 'structure Bracket { /* updated on disk */ }';
    vi.mocked(bridge.openFile).mockResolvedValue({
      path: '/project/bracket.ri',
      content: diskContent,
    });
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Set up dirty + externally-changed state
    capturedEditorStore.setActiveFile('/project/bracket.ri');
    capturedEditorStore.markDirty('/project/bracket.ri');
    capturedEditorStore.markExternallyChanged('/project/bracket.ri');

    // Trigger handleSave via Ctrl+S — should show conflict prompt (not blocked msg)
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    // Wait for the conflict toast with Reload action to appear
    let conflictToast: HTMLElement | undefined;
    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      conflictToast = toasts.find((t) =>
        t.textContent?.includes(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG),
      );
      expect(conflictToast).toBeTruthy();
    });

    // Click the "Reload from disk" action button
    const reloadBtn = within(conflictToast!).getByRole('button', { name: SAVE_CONFLICT_RELOAD_LABEL });
    fireEvent.click(reloadBtn);

    // bridgeOpenFile must be called with the file's path
    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/bracket.ri');
    });

    // After the reload promise resolves, the store's content is the new disk content
    await waitFor(() => {
      const file = capturedEditorStore.state.openFiles.find(
        (f: any) => f.path === '/project/bracket.ri',
      );
      expect(file?.content).toBe(diskContent);
    });

    // markClean was called — both dirtyFiles and externallyChanged are cleared
    await waitFor(() => {
      expect(capturedEditorStore.state.dirtyFiles).not.toContain('/project/bracket.ri');
      expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');
    });

    // bridgeSaveFile must NOT have been called
    expect(bridge.saveFile).not.toHaveBeenCalled();
  });
});

// ─── Conflict prompt: Overwrite action ───────────────────────────────────────

describe('App handleSave conflict prompt: Overwrite', () => {
  const testState = makeGuiState({
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
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

  it('clicking Overwrite calls bridgeSaveFile with the buffer content and clears dirty/externallyChanged without calling bridgeOpenFile', async () => {
    const bufferContent = 'structure Bracket { param width = 100mm }'; // user's edited content
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/project/bracket.ri', content: 'unused' });

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Set up dirty + externally-changed state with custom buffer content
    capturedEditorStore.setActiveFile('/project/bracket.ri');
    capturedEditorStore.markDirty('/project/bracket.ri');
    capturedEditorStore.markExternallyChanged('/project/bracket.ri');
    // Update the store's buffer content to simulate user edits
    capturedEditorStore.updateFileContent('/project/bracket.ri', bufferContent);

    // Trigger handleSave via Ctrl+S — shows conflict prompt
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    // Wait for the conflict toast with Overwrite button to appear
    let conflictToast: HTMLElement | undefined;
    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      conflictToast = toasts.find((t) =>
        t.textContent?.includes(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG),
      );
      expect(conflictToast).toBeTruthy();
    });

    // Click the "Overwrite" action button
    const overwriteBtn = within(conflictToast!).getByRole('button', { name: SAVE_CONFLICT_OVERWRITE_LABEL });
    fireEvent.click(overwriteBtn);

    // bridgeSaveFile must be called with (path, currentBufferContent)
    await waitFor(() => {
      expect(bridge.saveFile).toHaveBeenCalledWith(
        '/project/bracket.ri',
        bufferContent,
      );
    });

    // After save promise resolves, markClean was called — both flags cleared
    await waitFor(() => {
      expect(capturedEditorStore.state.dirtyFiles).not.toContain('/project/bracket.ri');
      expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');
    });

    // bridgeOpenFile must NOT have been called
    expect(bridge.openFile).not.toHaveBeenCalled();
  });
});

// ─── Conflict prompt: Overwrite uses live buffer content ─────────────────────

describe('App handleSave conflict prompt: Overwrite uses live buffer', () => {
  const testState = makeGuiState({
    files: [
      { path: '/project/bracket.ri', content: 'STALE' },
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

  it('clicking Overwrite saves the live buffer content (from liveContentRef), not the stale store snapshot', async () => {
    // The conflict prompt is created when Ctrl+S fires on an externally-changed
    // file.  The Overwrite action must save the live CM buffer, not the stale
    // store snapshot that was captured at open time.
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/project/bracket.ri', content: 'unused' });

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    await waitFor(() => expect(capturedEditorLiveContentRef).toBeDefined());

    // Set up dirty + externally-changed state; store snapshot stays 'STALE'
    capturedEditorStore.setActiveFile('/project/bracket.ri');
    capturedEditorStore.markDirty('/project/bracket.ri');
    capturedEditorStore.markExternallyChanged('/project/bracket.ri');

    // Wire the live content getter to simulate the user having typed
    capturedEditorLiveContentRef!(() => 'LIVE-OVERWRITE');

    // Trigger handleSave via Ctrl+S — shows conflict prompt (file is externally-changed)
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    // Wait for the conflict toast with Overwrite button to appear
    let conflictToast: HTMLElement | undefined;
    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      conflictToast = toasts.find((t) =>
        t.textContent?.includes(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG),
      );
      expect(conflictToast).toBeTruthy();
    });

    // Simulate a tab-switch after the prompt appears: mutate the getter so any
    // click-time read would return the wrong file's content.  The snapshot must
    // have been taken at prompt-CREATION time (when Ctrl+S fired), so Overwrite
    // should still save 'LIVE-OVERWRITE', not 'WRONG-FILE-AFTER-SWITCH'.
    capturedEditorLiveContentRef!(() => 'WRONG-FILE-AFTER-SWITCH');

    // Click the Overwrite action button
    const overwriteBtn = within(conflictToast!).getByRole('button', { name: SAVE_CONFLICT_OVERWRITE_LABEL });
    fireEvent.click(overwriteBtn);

    // bridgeSaveFile must be called with the LIVE buffer content, not 'STALE'
    // and not the post-switch 'WRONG-FILE-AFTER-SWITCH'.
    await waitFor(() => {
      expect(vi.mocked(bridge.saveFile)).toHaveBeenCalledWith(
        '/project/bracket.ri',
        'LIVE-OVERWRITE',
      );
    });
  });
});

// ─── Save-conflict resolution clears the reload-prompt banner ────────────────

describe('App save-conflict resolution clears the reload-prompt banner', () => {
  const testState = makeGuiState({
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
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

  it('(a) Reload from disk action clears the changedFiles banner after resolving the conflict', async () => {
    // Setup: dirty file + file-changed event → changedFiles + externallyChanged both set.
    // Guards: banner must appear, conflict prompt must surface via Ctrl+S,
    // then clicking Reload from disk must clear the banner (changedFiles cleared).
    const diskContent = '/* updated on disk */';
    vi.mocked(bridge.openFile).mockResolvedValue({
      path: '/project/bracket.ri',
      content: diskContent,
    });
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Mark dirty first so the onFileChanged handler takes the dirty branch
    capturedEditorStore.setActiveFile('/project/bracket.ri');
    capturedEditorStore.markDirty('/project/bracket.ri');

    // Fire file-changed: dirty branch → adds to changedFiles AND calls markExternallyChanged
    fileChangedCallback!({ path: '/project/bracket.ri', content: diskContent });

    // Banner must become visible (changedFiles is non-empty)
    await waitFor(() => {
      // data-testid="reload-prompt" is rendered by ReloadPrompt when filePaths.length > 0
      expect(screen.queryByTestId('reload-prompt')).not.toBeNull();
    });

    // externallyChanged must also be set (both signals populated by the dirty branch)
    expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri');

    // Trigger handleSave via Ctrl+S → conflict prompt appears (externallyChanged is set)
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    let conflictToast: HTMLElement | undefined;
    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      conflictToast = toasts.find((t) =>
        t.textContent?.includes(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG),
      );
      expect(conflictToast).toBeTruthy();
    });

    // Click "Reload from disk" → reloadFromDisk: bridgeOpenFile + updateFileContent + markClean
    const reloadBtn = within(conflictToast!).getByRole('button', { name: SAVE_CONFLICT_RELOAD_LABEL });
    fireEvent.click(reloadBtn);

    // bridgeOpenFile was called with the correct path
    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/bracket.ri');
    });

    // After reloadFromDisk resolves: the banner must be gone (changedFiles cleared).
    // This guards against banner staleness: reloadFromDisk must also call
    // setChangedFiles((prev) => { next.delete(path); return next; }) in addition to markClean.
    await waitFor(() => {
      expect(screen.queryByTestId('reload-prompt')).toBeNull();
    });

    // Store state is also clear
    await waitFor(() => {
      expect(capturedEditorStore.state.dirtyFiles).not.toContain('/project/bracket.ri');
      expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');
    });

    // Overwrite was NOT triggered
    expect(bridge.saveFile).not.toHaveBeenCalled();
  });

  it('(b) Overwrite action clears the changedFiles banner after resolving the conflict', async () => {
    // Same setup as (a) but clicks Overwrite — bridgeSaveFile must be called and
    // the banner must disappear (changedFiles cleared), bridgeOpenFile NOT called.
    const bufferContent = 'structure Bracket { param width = 100mm }';
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/project/bracket.ri', content: 'unused' });

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Mark dirty, fire file-changed → changedFiles + externallyChanged
    capturedEditorStore.setActiveFile('/project/bracket.ri');
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'newer disk content' });

    // Banner appears
    await waitFor(() => {
      expect(screen.queryByTestId('reload-prompt')).not.toBeNull();
    });

    // Set the buffer content that the user edited
    capturedEditorStore.updateFileContent('/project/bracket.ri', bufferContent);

    // Ctrl+S → conflict prompt
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    let conflictToast: HTMLElement | undefined;
    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      conflictToast = toasts.find((t) =>
        t.textContent?.includes(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG),
      );
      expect(conflictToast).toBeTruthy();
    });

    // Click "Overwrite"
    const overwriteBtn = within(conflictToast!).getByRole('button', { name: SAVE_CONFLICT_OVERWRITE_LABEL });
    fireEvent.click(overwriteBtn);

    // bridgeSaveFile called with the buffer content
    await waitFor(() => {
      expect(bridge.saveFile).toHaveBeenCalledWith('/project/bracket.ri', bufferContent);
    });

    // After overwriteFile resolves: banner must be gone (changedFiles cleared).
    // Guards against banner staleness: overwriteFile must also call
    // setChangedFiles((prev) => { next.delete(path); return next; }) in addition to markClean.
    await waitFor(() => {
      expect(screen.queryByTestId('reload-prompt')).toBeNull();
    });

    // Store state cleared
    await waitFor(() => {
      expect(capturedEditorStore.state.dirtyFiles).not.toContain('/project/bracket.ri');
      expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');
    });

    // Reload NOT called
    expect(bridge.openFile).not.toHaveBeenCalled();
  });
});

// ─── file-removed event handling (step-23) ───────────────────────────────────

describe('App file-removed event handling', () => {
  const testState = makeGuiState({
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
    ],
  });

  let fileRemovedCallback: ((data: { path: string }) => void) | undefined;

  beforeEach(() => {
    fileRemovedCallback = undefined;
    vi.mocked((bridge as any).onFileRemoved).mockImplementation(async (cb: any) => {
      fileRemovedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));
  });

  it('(a) onFileRemoved is subscribed to during initApp', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileRemovedCallback).toBeDefined());
    expect(bridge.onFileRemoved).toHaveBeenCalled();
  });

  it('(b) firing with a path in openFiles adds it to missingFiles', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileRemovedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    fileRemovedCallback!({ path: '/project/bracket.ri' });

    await waitFor(() => {
      expect(capturedEditorStore.state.missingFiles).toContain('/project/bracket.ri');
    });
  });

  it('(c) firing with a path NOT in openFiles does NOT add to missingFiles', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileRemovedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    fileRemovedCallback!({ path: '/project/other.ri' });

    await new Promise((r) => setTimeout(r, 10));
    expect(capturedEditorStore.state.missingFiles).not.toContain('/project/other.ri');
  });

  it('(d) unsub from onFileRemoved is called on App unmount', async () => {
    const fileRemovedUnsub = vi.fn();
    vi.mocked((bridge as any).onFileRemoved).mockResolvedValue(fileRemovedUnsub);

    const { unmount } = render(() => <App />);
    await waitFor(() => expect(screen.getByTestId('app-layout')).toBeTruthy());

    unmount();
    expect(fileRemovedUnsub).toHaveBeenCalled();
  });
});
