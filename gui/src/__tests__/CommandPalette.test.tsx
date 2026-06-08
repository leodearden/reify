import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@solidjs/testing-library';
import type { DocumentSymbol } from '../editor/lspClient';
import type { PaletteCommand } from '../hooks/useKeyboardShortcuts';
import { CommandPalette } from '../components/CommandPalette';

// ── Test fixtures ─────────────────────────────────────────────────────────────

const COMMANDS: PaletteCommand[] = [
  { id: 'save',       title: 'Save file',        key: 'Ctrl+S' },
  { id: 'toggleChat', title: 'Toggle chat panel', key: 'Ctrl+J' },
];

const MOCK_SYMBOLS: DocumentSymbol[] = [
  {
    name: 'Bracket',
    kind: 5,
    range: { start: { line: 0, character: 0 }, end: { line: 10, character: 1 } },
    selectionRange: { start: { line: 0, character: 10 }, end: { line: 0, character: 17 } },
    children: [
      {
        name: 'width',
        kind: 13,
        range: { start: { line: 1, character: 2 }, end: { line: 1, character: 14 } },
        selectionRange: { start: { line: 1, character: 2 }, end: { line: 1, character: 7 } },
      },
    ],
  },
];

// ── Helpers ───────────────────────────────────────────────────────────────────

describe('CommandPalette', () => {
  let runCommand: ReturnType<typeof vi.fn>;
  let fetchSymbols: ReturnType<typeof vi.fn>;
  let onJumpToLocation: ReturnType<typeof vi.fn>;
  let onClose: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    runCommand = vi.fn();
    fetchSymbols = vi.fn().mockResolvedValue(MOCK_SYMBOLS);
    onJumpToLocation = vi.fn();
    onClose = vi.fn();
  });

  function renderPalette(initialMode?: 'command' | 'symbol') {
    return render(() => (
      <CommandPalette
        getCommands={() => COMMANDS}
        runCommand={runCommand}
        fetchSymbols={fetchSymbols}
        filePath="main.ri"
        onJumpToLocation={onJumpToLocation}
        onClose={onClose}
        initialMode={initialMode}
      />
    ));
  }

  // ── (a) renders input and command rows ─────────────────────────────────────

  it('renders a text input and all command rows by default', () => {
    renderPalette();
    expect(screen.getByRole('textbox')).toBeTruthy();
    expect(screen.getByText('Save file')).toBeTruthy();
    expect(screen.getByText('Toggle chat panel')).toBeTruthy();
  });

  it('renders the keyboard shortcut key beside each command', () => {
    renderPalette();
    expect(screen.getByText('Ctrl+S')).toBeTruthy();
    expect(screen.getByText('Ctrl+J')).toBeTruthy();
  });

  // ── (b) typing filters the command list ────────────────────────────────────

  it('typing a query filters the command list to matching entries', async () => {
    renderPalette();
    const input = screen.getByRole('textbox');
    fireEvent.input(input, { target: { value: 'save' } });
    await waitFor(() => {
      expect(screen.getByText('Save file')).toBeTruthy();
      expect(screen.queryByText('Toggle chat panel')).toBeNull();
    });
  });

  it('clearing the query restores all commands', async () => {
    renderPalette();
    const input = screen.getByRole('textbox');
    fireEvent.input(input, { target: { value: 'save' } });
    await waitFor(() => expect(screen.queryByText('Toggle chat panel')).toBeNull());
    fireEvent.input(input, { target: { value: '' } });
    await waitFor(() => expect(screen.getByText('Toggle chat panel')).toBeTruthy());
  });

  // ── (c) Enter runs the selected command ────────────────────────────────────

  it('pressing Enter on the first row runs that command and calls onClose', () => {
    renderPalette();
    const input = screen.getByRole('textbox');
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(runCommand).toHaveBeenCalledWith('save');
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('ArrowDown then Enter runs the second command and calls onClose', () => {
    renderPalette();
    const input = screen.getByRole('textbox');
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(runCommand).toHaveBeenCalledWith('toggleChat');
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('ArrowUp from the first row wraps around or stays at first', () => {
    renderPalette();
    const input = screen.getByRole('textbox');
    // ArrowUp from index 0 should not crash
    fireEvent.keyDown(input, { key: 'ArrowUp' });
    fireEvent.keyDown(input, { key: 'Enter' });
    // Should still run a command (either first or last — implementation choice)
    expect(runCommand).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  // ── (d) '@' switches to symbol mode ───────────────────────────────────────

  it('typing "@" switches to symbol mode and renders flattened symbol rows', async () => {
    renderPalette();
    const input = screen.getByRole('textbox');
    fireEvent.input(input, { target: { value: '@' } });
    await waitFor(() => {
      expect(fetchSymbols).toHaveBeenCalled();
      expect(screen.getByText('Bracket')).toBeTruthy();
      expect(screen.getByText('width')).toBeTruthy();
    });
  });

  it('typing a query after "@" fuzzy-filters the symbol list', async () => {
    renderPalette();
    const input = screen.getByRole('textbox');
    fireEvent.input(input, { target: { value: '@wid' } });
    await waitFor(() => {
      expect(screen.getByText('width')).toBeTruthy();
      expect(screen.queryByText('Bracket')).toBeNull();
    });
  });

  // ── (e) Enter on a symbol calls onJumpToLocation ───────────────────────────

  it('Enter in symbol mode calls onJumpToLocation with 1-based SourceLocation and onClose', async () => {
    renderPalette();
    const input = screen.getByRole('textbox');
    fireEvent.input(input, { target: { value: '@' } });
    await waitFor(() => expect(screen.getByText('Bracket')).toBeTruthy());

    fireEvent.keyDown(input, { key: 'Enter' });

    // Bracket selectionRange.start = { line: 0, character: 10 } → 1-based { line: 1, column: 11 }
    expect(onJumpToLocation).toHaveBeenCalledWith(
      expect.objectContaining({
        file_path: 'main.ri',
        line: 1,
        column: 11,
        end_line: 1,
        end_column: 11,
      }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('ArrowDown then Enter in symbol mode jumps to the second flattened symbol', async () => {
    renderPalette();
    const input = screen.getByRole('textbox');
    fireEvent.input(input, { target: { value: '@' } });
    await waitFor(() => expect(screen.getByText('width')).toBeTruthy());

    fireEvent.keyDown(input, { key: 'ArrowDown' });
    fireEvent.keyDown(input, { key: 'Enter' });

    // Second flattened symbol = 'width', selectionRange.start = { line: 1, character: 2 } → 1-based { line: 2, column: 3 }
    expect(onJumpToLocation).toHaveBeenCalledWith(
      expect.objectContaining({
        file_path: 'main.ri',
        line: 2,
        column: 3,
      }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  // ── (f) initialMode:'symbol' fetches symbols on mount ─────────────────────

  it('initialMode "symbol" calls fetchSymbols immediately and shows symbols', async () => {
    renderPalette('symbol');
    await waitFor(() => {
      expect(fetchSymbols).toHaveBeenCalled();
      expect(screen.getByText('Bracket')).toBeTruthy();
    });
  });

  it('initialMode "symbol" does not show command rows', async () => {
    renderPalette('symbol');
    await waitFor(() => expect(screen.getByText('Bracket')).toBeTruthy());
    // Command rows should not be visible in symbol mode
    expect(screen.queryByText('Save file')).toBeNull();
  });

  // ── (g) Escape calls onClose ───────────────────────────────────────────────

  it('pressing Escape calls onClose', () => {
    renderPalette();
    const input = screen.getByRole('textbox');
    fireEvent.keyDown(input, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(runCommand).not.toHaveBeenCalled();
  });

  // ── failure modes ──────────────────────────────────────────────────────────

  it('does not crash and does not call onClose when fetchSymbols rejects', async () => {
    const rejectingFetch = vi.fn().mockRejectedValue(new Error('LSP error'));
    const { unmount } = render(() => (
      <CommandPalette
        getCommands={() => COMMANDS}
        runCommand={runCommand}
        fetchSymbols={rejectingFetch}
        filePath="main.ri"
        onJumpToLocation={onJumpToLocation}
        onClose={onClose}
        initialMode="symbol"
      />
    ));
    await waitFor(() => expect(rejectingFetch).toHaveBeenCalled());
    // The palette must remain open; onClose should NOT have been called.
    expect(onClose).not.toHaveBeenCalled();
    // Enter on the empty/error list should be a no-op (no jump, no close).
    const input = screen.getByRole('textbox');
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onJumpToLocation).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    unmount();
  });

  it('shows empty state and Enter is a no-op when fetchSymbols resolves to []', async () => {
    const emptyFetch = vi.fn().mockResolvedValue([]);
    render(() => (
      <CommandPalette
        getCommands={() => COMMANDS}
        runCommand={runCommand}
        fetchSymbols={emptyFetch}
        filePath="main.ri"
        onJumpToLocation={onJumpToLocation}
        onClose={onClose}
        initialMode="symbol"
      />
    ));
    await waitFor(() => expect(emptyFetch).toHaveBeenCalled());
    // An empty-state indicator should be visible.
    await waitFor(() => expect(screen.getByText('No symbols found')).toBeTruthy());
    // Enter on the empty list is a no-op.
    const input = screen.getByRole('textbox');
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onJumpToLocation).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });
});
