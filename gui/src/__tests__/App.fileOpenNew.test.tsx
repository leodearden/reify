import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@solidjs/testing-library';
import { flushMacrotasks, withSuppressedRejections } from './test-utils';
import { published } from './test_utils/publishedState';
import { makeGuiState } from './test_utils/guiState';

vi.mock('@tauri-apps/api/core', async () => (await import('./appHarness/moduleMocks')).tauriCoreMock());
vi.mock('@tauri-apps/api/event', async () => (await import('./appHarness/moduleMocks')).tauriEventMock());
vi.mock('../viewport', async () => (await import('./appHarness/renderStubs')).viewportStub());
vi.mock('../editor/Editor', async () => (await import('./appHarness/renderStubs')).editorStub());
vi.mock('../editor/FileTabs', async () => (await import('./appHarness/renderStubs')).fileTabsStub());
vi.mock('../bridge', async () => (await import('./appHarness/moduleMocks')).appBridgeMock());
vi.mock('../debug', async () => (await import('./appHarness/moduleMocks')).debugMock());
vi.mock('../stores/sidecarPersistence', async () => (await import('./appHarness/moduleMocks')).sidecarPersistenceMock());
vi.mock('../stores/viewPersistence', async (importOriginal) => (await import('./appHarness/moduleMocks')).viewPersistenceMock(await importOriginal()));

import App from '../App';
import * as bridge from '../bridge';
import { primeAppHarness, renderAndWaitForReady } from './appHarness/harness';
import { capturedEditorStore, capturedEditorOnOpen } from './appHarness/renderStubs';

beforeEach(primeAppHarness);

describe('App file picker integration (E-6)', () => {
  it('calls pickSavePath then exportGeometry with the chosen path', async () => {
    vi.mocked(bridge.pickSavePath).mockResolvedValue('/user/chosen/export.step');
    vi.mocked(bridge.exportGeometry).mockResolvedValue(undefined);

    await renderAndWaitForReady();

    // Open the export dialog
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => {
      expect(screen.getByTestId('export-dialog')).toBeTruthy();
    });

    // Click Export inside the dialog (default format is 'step')
    const dialog = screen.getByTestId('export-dialog');
    const exportBtn = dialog.querySelector('button:last-of-type') as HTMLButtonElement;
    fireEvent.click(exportBtn);

    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalledWith('export.step', 'step');
    });

    await waitFor(() => {
      expect(bridge.exportGeometry).toHaveBeenCalledWith('step', '/user/chosen/export.step');
    });
  });

  it('does NOT call exportGeometry when user cancels file picker', async () => {
    vi.mocked(bridge.pickSavePath).mockResolvedValue(null);
    vi.mocked(bridge.exportGeometry).mockResolvedValue(undefined);

    await renderAndWaitForReady();

    // Open the export dialog
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => {
      expect(screen.getByTestId('export-dialog')).toBeTruthy();
    });

    // Click Export inside the dialog
    const dialog = screen.getByTestId('export-dialog');
    const exportBtn = dialog.querySelector('button:last-of-type') as HTMLButtonElement;
    fireEvent.click(exportBtn);

    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalled();
    });

    // Give time for any downstream calls
    await new Promise((r) => setTimeout(r, 50));

    // exportGeometry should NOT have been called
    expect(bridge.exportGeometry).not.toHaveBeenCalled();

    // Dialog should still be open (not closed after cancel)
    expect(screen.getByTestId('export-dialog')).toBeTruthy();
  });
});

describe('App pickSavePath error boundary', () => {
  it('shows error toast and keeps dialog open when pickSavePath rejects', async () => {
    vi.mocked(bridge.pickSavePath).mockRejectedValue(new Error('Plugin not registered'));
    vi.mocked(bridge.exportGeometry).mockResolvedValue(undefined);

    await renderAndWaitForReady();

    // Open the export dialog
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => {
      expect(screen.getByTestId('export-dialog')).toBeTruthy();
    });

    // Click Export inside the dialog
    const dialog = screen.getByTestId('export-dialog');
    const exportBtn = dialog.querySelector('button:last-of-type') as HTMLButtonElement;
    fireEvent.click(exportBtn);

    // Wait for the rejection to be handled
    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalled();
    });

    // Give time for any async handling
    await new Promise((r) => setTimeout(r, 50));

    // (1) Error toast should be shown with message about save dialog failure
    await waitFor(() => {
      expect(screen.getByText(/Could not open save dialog/)).toBeTruthy();
    });

    // (2) bridgeExportGeometry should NOT have been called
    expect(bridge.exportGeometry).not.toHaveBeenCalled();

    // (3) Dialog should still be open and NOT in exporting state (no spinner)
    expect(screen.getByTestId('export-dialog')).toBeTruthy();
    expect(screen.queryByTestId('export-spinner')).toBeNull();
  });
});

describe('App Ctrl+O open file', () => {
  it('App wires handleOpen into Editor.onOpen (invoking it calls bridge.pickOpenPath)', async () => {
    const pickSpy = vi.spyOn(bridge, 'pickOpenPath').mockResolvedValue(null);
    await renderAndWaitForReady();

    // capturedEditorOnOpen should be wired to App's handleOpen
    await vi.waitFor(() => {
      expect(capturedEditorOnOpen).toBeDefined();
    });

    // Invoke the captured callback — should trigger bridge.pickOpenPath
    capturedEditorOnOpen!();

    await vi.waitFor(() => {
      expect(pickSpy).toHaveBeenCalledTimes(1);
    });
  });

  it('dispatching Ctrl+O triggers pickOpenPath then openFile', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
    })));

    // Mock pickOpenPath to return a path
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/project/other.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/project/other.ri', content: 'structure Other {}' });

    render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // Press Ctrl+O
    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });

    // Should call pickOpenPath, then openFile with the returned path
    await waitFor(() => {
      expect(bridge.pickOpenPath).toHaveBeenCalled();
    });

    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/other.ri');
    });
    await waitFor(() => {
      expect(bridge.recordRecent).toHaveBeenCalledWith('/project/other.ri');
    });
  });
});

describe('App handleOpen dirty-check confirmation', () => {
  function setupHappyPathMocks() {
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/project/other.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/project/other.ri', content: 'structure Other {}' });
    vi.mocked(bridge.openFileEngine).mockResolvedValue(published(makeGuiState()));
  }

  it('Ctrl+O with dirty buffer and confirm cancelled: pickOpenPath not called', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    capturedEditorStore.markDirty('/project/bracket.ri');

    vi.mocked((bridge as any).ask).mockResolvedValue(false);
    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
    await flushMacrotasks();
    expect(bridge.ask).toHaveBeenCalledTimes(1);
    expect(bridge.pickOpenPath).not.toHaveBeenCalled();
    expect(bridge.openFile).not.toHaveBeenCalled();
    expect(bridge.openFileEngine).not.toHaveBeenCalled();
  });

  it('Ctrl+O with dirty buffer and confirm accepted: bridge sequence fires', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    capturedEditorStore.markDirty('/project/bracket.ri');

    vi.mocked((bridge as any).ask).mockResolvedValue(true);
    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
    await waitFor(() => {
      expect(bridge.pickOpenPath).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/other.ri');
    });
  });

  it('Ctrl+O with dirty buffer, confirm accepted, and pickOpenPath returns null: openFile/openFileEngine not called', async () => {
    setupHappyPathMocks();
    vi.mocked(bridge.pickOpenPath).mockResolvedValue(null);
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    capturedEditorStore.markDirty('/project/bracket.ri');

    vi.mocked((bridge as any).ask).mockResolvedValue(true);
    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
    await flushMacrotasks();
    expect(bridge.ask).toHaveBeenCalledTimes(1);
    expect(bridge.pickOpenPath).toHaveBeenCalledTimes(1);
    expect(bridge.openFile).not.toHaveBeenCalled();
    expect(bridge.openFileEngine).not.toHaveBeenCalled();
  });

  it('Ctrl+O with clean buffer: bridge.ask not called, bridge sequence fires', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    // No markDirty — buffer is clean

    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
    await waitFor(() => {
      expect(bridge.pickOpenPath).toHaveBeenCalled();
    });
    expect(bridge.ask).not.toHaveBeenCalled();
  });
});

describe('App File→New (Ctrl+N) create-and-open flow', () => {
  const newPath = '/user/chosen/new.ri';
  const newContent = '// New design\n';

  function setupHappyPathMocks() {
    vi.mocked(bridge.pickSavePath).mockResolvedValue(newPath);
    vi.mocked(bridge.createNewFile).mockResolvedValue({ path: newPath, content: newContent });
    vi.mocked(bridge.openFile).mockResolvedValue({ path: newPath, content: newContent });
    vi.mocked(bridge.openFileEngine).mockResolvedValue(published(makeGuiState()));
  }

  it('Ctrl+N happy path: calls pickSavePath, createNewFile, openFile, openFileEngine in order', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();

    fireEvent.keyDown(document, { key: 'n', ctrlKey: true });

    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalledWith('untitled.ri', 'ri');
    });
    await waitFor(() => {
      expect(bridge.createNewFile).toHaveBeenCalledWith(newPath);
    });
    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith(newPath);
    });
    await waitFor(() => {
      expect(bridge.openFileEngine).toHaveBeenCalledWith(newPath);
    });
  });

  it('File→New menu click: calls the same bridge sequence', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();

    fireEvent.click(screen.getByText('File'));
    const newItem = screen.getAllByRole('menuitem').find((el) => el.textContent?.includes('New'));
    expect(newItem).toBeTruthy();
    fireEvent.click(newItem!);

    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalledWith('untitled.ri', 'ri');
    });
    await waitFor(() => {
      expect(bridge.createNewFile).toHaveBeenCalledWith(newPath);
    });
    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith(newPath);
    });
  });

  it('cancel: pickSavePath returns null → createNewFile and openFile not called', async () => {
    vi.mocked(bridge.pickSavePath).mockResolvedValue(null);
    await renderAndWaitForReady();

    fireEvent.keyDown(document, { key: 'n', ctrlKey: true });

    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalled();
    });
    await flushMacrotasks();

    expect(bridge.createNewFile).not.toHaveBeenCalled();
    expect(bridge.openFile).not.toHaveBeenCalled();
    expect(bridge.openFileEngine).not.toHaveBeenCalled();
  });

  it('error: pickSavePath rejects → shows "New file failed" toast, createNewFile not called', async () => {
    await withSuppressedRejections(async () => {
      vi.mocked(bridge.pickSavePath).mockRejectedValue(new Error('Plugin not registered'));
      await renderAndWaitForReady();

      fireEvent.keyDown(document, { key: 'n', ctrlKey: true });

      await waitFor(() => {
        expect(screen.getByText(/New file failed/)).toBeTruthy();
      });
      expect(bridge.createNewFile).not.toHaveBeenCalled();
    });
  });

  it('error: createNewFile rejects → shows a coded "New file failed" toast with the error message, openFile not called', async () => {
    await withSuppressedRejections(async () => {
      vi.mocked(bridge.pickSavePath).mockResolvedValue(newPath);
      vi.mocked(bridge.createNewFile).mockRejectedValue(new Error('disk full'));
      await renderAndWaitForReady();

      fireEvent.keyDown(document, { key: 'n', ctrlKey: true });

      await waitFor(() => {
        expect(screen.getByText(/New file failed/)).toBeTruthy();
      });
      await waitFor(() => {
        expect(screen.getByText(/E_GUI_TEMPLATE_WRITE_FAILED: disk full/)).toBeTruthy();
      });
      expect(bridge.openFile).not.toHaveBeenCalled();
    });
  });

  it('editor store integration: after successful new-file flow, openFiles contains new path', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    fireEvent.keyDown(document, { key: 'n', ctrlKey: true });

    await waitFor(() => {
      const paths = capturedEditorStore.state.openFiles.map((f: any) => f.path);
      expect(paths).toContain(newPath);
    });
  });
});

describe('App handleNew dirty-check confirmation', () => {
  const newPath = '/user/chosen/new.ri';
  const newContent = '// New design\n';

  function setupHappyPathMocks() {
    vi.mocked(bridge.pickSavePath).mockResolvedValue(newPath);
    vi.mocked(bridge.createNewFile).mockResolvedValue({ path: newPath, content: newContent });
    vi.mocked(bridge.openFile).mockResolvedValue({ path: newPath, content: newContent });
    vi.mocked(bridge.openFileEngine).mockResolvedValue(published(makeGuiState()));
  }

  it('Ctrl+N with dirty buffer and confirm cancelled: pickSavePath not called', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    capturedEditorStore.markDirty('/project/bracket.ri');

    vi.mocked((bridge as any).ask).mockResolvedValue(false);
    fireEvent.keyDown(document, { key: 'n', ctrlKey: true });
    await flushMacrotasks();
    expect(bridge.ask).toHaveBeenCalledTimes(1);
    expect(bridge.pickSavePath).not.toHaveBeenCalled();
    expect(bridge.createNewFile).not.toHaveBeenCalled();
    expect(bridge.openFile).not.toHaveBeenCalled();
    expect(bridge.openFileEngine).not.toHaveBeenCalled();
  });

  it('Ctrl+N with dirty buffer and confirm accepted: bridge sequence fires', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    capturedEditorStore.markDirty('/project/bracket.ri');

    vi.mocked((bridge as any).ask).mockResolvedValue(true);
    fireEvent.keyDown(document, { key: 'n', ctrlKey: true });
    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalledWith('untitled.ri', 'ri');
    });
    await waitFor(() => {
      expect(bridge.createNewFile).toHaveBeenCalledWith(newPath);
    });
    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith(newPath);
    });
    await waitFor(() => {
      expect(bridge.openFileEngine).toHaveBeenCalledWith(newPath);
    });
  });

  it('Ctrl+N with clean buffer: bridge.ask not called, bridge sequence fires', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    // No markDirty — buffer is clean

    fireEvent.keyDown(document, { key: 'n', ctrlKey: true });
    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalledWith('untitled.ri', 'ri');
    });
    expect(bridge.ask).not.toHaveBeenCalled();
  });
});
