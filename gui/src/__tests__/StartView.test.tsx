import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@solidjs/testing-library';
import { StartView } from '../panels/StartView';
import { createLauncherStore, LAUNCH_ERROR_CODES, type LauncherStore } from '../stores/launcherStore';
import { LaunchFailure, type LauncherActions } from '../app/launcherActions';
import { pickOpenPath, pickSavePath } from '../bridge';

vi.mock('../bridge', () => ({
  pickOpenPath: vi.fn(),
  pickSavePath: vi.fn(),
}));

const mockPickOpenPath = vi.mocked(pickOpenPath);
const mockPickSavePath = vi.mocked(pickSavePath);

function makeActions() {
  return {
    openDocument: vi.fn<LauncherActions['openDocument']>().mockResolvedValue(undefined),
    createDocument: vi.fn<LauncherActions['createDocument']>().mockResolvedValue(undefined),
    refreshRecent: vi.fn<LauncherActions['refreshRecent']>().mockResolvedValue(undefined),
    selectMode: vi.fn<LauncherActions['selectMode']>().mockResolvedValue(undefined),
  };
}

let store: LauncherStore;
let actions: ReturnType<typeof makeActions>;

function renderStartView() {
  return render(() => <StartView launcher={store} actions={actions} />);
}

/** Let every pending promise chain started by a click settle. */
function flush(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function typePath(path: string) {
  fireEvent.input(screen.getByTestId('start-path-input'), { target: { value: path } });
}

/** The element a control names as its description through `aria-describedby`, if any. */
function descriptionOf(control: HTMLElement): HTMLElement | null {
  const id = control.getAttribute('aria-describedby');
  return id === null ? null : document.getElementById(id);
}

beforeEach(() => {
  store = createLauncherStore();
  actions = makeActions();
  mockPickOpenPath.mockReset().mockResolvedValue(null);
  mockPickSavePath.mockReset().mockResolvedValue(null);
});

describe('StartView layout', () => {
  it('renders the start view, which is not a dialog', () => {
    renderStartView();

    const root = screen.getByTestId('start-view');
    expect(root.getAttribute('role')).not.toBe('dialog');
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('refreshes the recent list on mount', () => {
    renderStartView();

    expect(actions.refreshRecent).toHaveBeenCalledTimes(1);
  });

  it('presses only the selected mode button', () => {
    renderStartView();

    expect(screen.getByTestId('start-mode-design').getAttribute('aria-pressed')).toBe('false');
    expect(screen.getByTestId('start-mode-assistant').getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByTestId('start-mode-debug').getAttribute('aria-pressed')).toBe('false');

    store.applyModeState({ mode: 'debug', debug_port: 3939 });

    expect(screen.getByTestId('start-mode-assistant').getAttribute('aria-pressed')).toBe('false');
    expect(screen.getByTestId('start-mode-debug').getAttribute('aria-pressed')).toBe('true');
  });

  it('notes that the assistant needs a Claude Code login', () => {
    renderStartView();

    expect(screen.getByTestId('start-assistant-note').textContent).toContain('Claude Code');
  });

  it('notes beside the Debug option that it opens a local control port, before debug is selected', () => {
    renderStartView();

    const note = screen.getByTestId('start-debug-note');
    expect(note.textContent).toContain('127.0.0.1');
    expect(note.textContent).toContain('control port');
    expect(screen.getByTestId('start-mode-debug').closest('section')?.contains(note)).toBe(true);

    store.applyModeState({ mode: 'debug', debug_port: 3939 });

    expect(screen.getByTestId('start-debug-note')).toBeTruthy();
  });

  it('describes the Assistant and Debug buttons by their notes, so a screen reader announces them', () => {
    renderStartView();

    expect(descriptionOf(screen.getByTestId('start-mode-assistant'))).toBe(screen.getByTestId('start-assistant-note'));
    expect(descriptionOf(screen.getByTestId('start-mode-debug'))).toBe(screen.getByTestId('start-debug-note'));
    expect(screen.getByTestId('start-mode-design').hasAttribute('aria-describedby')).toBe(false);
  });

  it('shows the debug port only once the debug server is live', () => {
    renderStartView();
    expect(screen.queryByTestId('start-debug-port')).toBeNull();

    store.applyModeState({ mode: 'debug', debug_port: 3939 });

    expect(screen.getByTestId('start-debug-port').textContent).toContain('127.0.0.1:3939');
  });

  it('lists the recent entries in order, by path', () => {
    store.setRecent([
      { path: '/designs/bracket.ri', opened_at: 2000 },
      { path: '/designs/flange.ri', opened_at: 1000 },
    ]);
    renderStartView();

    const items = screen.getAllByTestId('start-recent-item');
    expect(items.map((item) => item.textContent)).toEqual([
      expect.stringContaining('/designs/bracket.ri'),
      expect.stringContaining('/designs/flange.ri'),
    ]);
  });
});

describe('StartView document actions', () => {
  it('Open… opens the path the dialog returned', async () => {
    mockPickOpenPath.mockResolvedValue('/designs/bracket.ri');
    renderStartView();

    fireEvent.click(screen.getByTestId('start-open-dialog'));

    await waitFor(() => expect(actions.openDocument).toHaveBeenCalledWith('/designs/bracket.ri'));
  });

  it('a cancelled Open… dialog opens nothing', async () => {
    renderStartView();

    fireEvent.click(screen.getByTestId('start-open-dialog'));

    await waitFor(() => expect(mockPickOpenPath).toHaveBeenCalledTimes(1));
    await flush();
    expect(actions.openDocument).not.toHaveBeenCalled();
  });

  it('New… asks for an untitled.ri save path and creates it', async () => {
    mockPickSavePath.mockResolvedValue('/designs/untitled.ri');
    renderStartView();

    fireEvent.click(screen.getByTestId('start-new-dialog'));

    await waitFor(() => expect(actions.createDocument).toHaveBeenCalledWith('/designs/untitled.ri'));
    expect(mockPickSavePath).toHaveBeenCalledWith('untitled.ri', 'ri');
  });

  it('a cancelled New… dialog creates nothing', async () => {
    renderStartView();

    fireEvent.click(screen.getByTestId('start-new-dialog'));

    await waitFor(() => expect(mockPickSavePath).toHaveBeenCalledTimes(1));
    await flush();
    expect(actions.createDocument).not.toHaveBeenCalled();
  });

  it('the typed path opens through Open and creates through Create', async () => {
    renderStartView();
    typePath('/designs/typed.ri');

    fireEvent.click(screen.getByTestId('start-open-path'));
    await waitFor(() => expect(actions.openDocument).toHaveBeenCalledWith('/designs/typed.ri'));

    fireEvent.click(screen.getByTestId('start-create-path'));
    await waitFor(() => expect(actions.createDocument).toHaveBeenCalledWith('/designs/typed.ri'));
  });

  it('an empty path opens and creates nothing', async () => {
    renderStartView();

    fireEvent.click(screen.getByTestId('start-open-path'));
    fireEvent.click(screen.getByTestId('start-create-path'));
    await flush();

    expect(actions.openDocument).not.toHaveBeenCalled();
    expect(actions.createDocument).not.toHaveBeenCalled();
  });

  it('clicking a recent entry opens its path', async () => {
    store.setRecent([
      { path: '/designs/bracket.ri', opened_at: 2000 },
      { path: '/designs/flange.ri', opened_at: 1000 },
    ]);
    renderStartView();

    fireEvent.click(screen.getAllByTestId('start-recent-item')[1]);

    await waitFor(() => expect(actions.openDocument).toHaveBeenCalledWith('/designs/flange.ri'));
  });

  it('a failed open shows its code and message as an alert and keeps the start view', async () => {
    actions.openDocument.mockRejectedValue(
      new LaunchFailure(LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED, 'No such file: /designs/missing.ri'),
    );
    renderStartView();
    typePath('/designs/missing.ri');

    fireEvent.click(screen.getByTestId('start-open-path'));

    const error = await screen.findByTestId('start-document-error');
    expect(error.getAttribute('role')).toBe('alert');
    expect(error.textContent).toContain('E_GUI_OPEN_FAILED');
    expect(error.textContent).toContain('No such file: /designs/missing.ri');
    expect(store.state.document.error?.code).toBe(LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED);
    expect(screen.getByTestId('start-view')).toBeTruthy();
  });

  it('a failed create shows E_GUI_TEMPLATE_WRITE_FAILED', async () => {
    actions.createDocument.mockRejectedValue(
      new LaunchFailure(LAUNCH_ERROR_CODES.E_GUI_TEMPLATE_WRITE_FAILED, 'File already exists'),
    );
    renderStartView();
    typePath('/designs/taken.ri');

    fireEvent.click(screen.getByTestId('start-create-path'));

    const error = await screen.findByTestId('start-document-error');
    expect(error.textContent).toContain('E_GUI_TEMPLATE_WRITE_FAILED');
  });

  it('an Open… dialog that fails to open shows E_GUI_OPEN_FAILED', async () => {
    mockPickOpenPath.mockRejectedValue(new Error('No dialog backend'));
    renderStartView();

    fireEvent.click(screen.getByTestId('start-open-dialog'));

    const error = await screen.findByTestId('start-document-error');
    expect(error.textContent).toContain('E_GUI_OPEN_FAILED');
    expect(error.textContent).toContain('No dialog backend');
    expect(actions.openDocument).not.toHaveBeenCalled();
  });

  it('a New… dialog that fails to open shows E_GUI_TEMPLATE_WRITE_FAILED', async () => {
    mockPickSavePath.mockRejectedValue(new Error('No dialog backend'));
    renderStartView();

    fireEvent.click(screen.getByTestId('start-new-dialog'));

    const error = await screen.findByTestId('start-document-error');
    expect(error.textContent).toContain('E_GUI_TEMPLATE_WRITE_FAILED');
    expect(error.textContent).toContain('No dialog backend');
    expect(actions.createDocument).not.toHaveBeenCalled();
  });

  it('an action rejection that is not a LaunchFailure is presented under the attempt\'s code', async () => {
    actions.openDocument.mockRejectedValue(new Error('unexpected'));
    store.setRecent([{ path: '/designs/bracket.ri', opened_at: 1000 }]);
    renderStartView();

    fireEvent.click(screen.getByTestId('start-recent-item'));

    const error = await screen.findByTestId('start-document-error');
    expect(error.textContent).toContain('E_GUI_OPEN_FAILED');
    expect(store.state.document.error).toEqual({
      code: LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED,
      message: 'unexpected',
    });
  });

  it('a new attempt clears the previous document error', async () => {
    actions.openDocument.mockRejectedValueOnce(
      new LaunchFailure(LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED, 'No such file'),
    );
    renderStartView();
    typePath('/designs/missing.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));
    await screen.findByTestId('start-document-error');

    typePath('/designs/bracket.ri');
    fireEvent.click(screen.getByTestId('start-open-path'));

    await waitFor(() => expect(screen.queryByTestId('start-document-error')).toBeNull());
  });
});

describe('StartView mode buttons', () => {
  it.each(['design', 'assistant', 'debug'] as const)('the %s button selects that mode', async (mode) => {
    renderStartView();

    fireEvent.click(screen.getByTestId(`start-mode-${mode}`));

    await waitFor(() => expect(actions.selectMode).toHaveBeenCalledWith(mode));
  });

  it('a failed mode selection shows E_GUI_DEBUG_LISTENER_FAILED', async () => {
    actions.selectMode.mockRejectedValue(
      new LaunchFailure(LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED, 'Address already in use'),
    );
    renderStartView();

    fireEvent.click(screen.getByTestId('start-mode-debug'));

    const error = await screen.findByTestId('start-mode-error');
    expect(error.textContent).toContain('E_GUI_DEBUG_LISTENER_FAILED');
    expect(store.state.mode.error?.code).toBe(LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED);
    expect(store.state.mode.selected).toBe('assistant');
  });

  it('a mode rejection that is not a LaunchFailure is presented under E_GUI_DEBUG_LISTENER_FAILED', async () => {
    actions.selectMode.mockRejectedValue(new Error('unexpected'));
    renderStartView();

    fireEvent.click(screen.getByTestId('start-mode-debug'));

    const error = await screen.findByTestId('start-mode-error');
    expect(error.textContent).toContain('E_GUI_DEBUG_LISTENER_FAILED');
    expect(store.state.mode.error).toEqual({
      code: LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED,
      message: 'unexpected',
    });
  });
});
