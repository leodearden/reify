import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { KeyboardHelp } from '../components/KeyboardHelp';
import { SHORTCUTS, getShortcut } from '../shortcuts';

describe('KeyboardHelp', () => {
  it('renders overlay with data-testid keyboard-help containing shortcut list', () => {
    render(() => <KeyboardHelp onClose={() => {}} />);
    const overlay = screen.getByTestId('keyboard-help');
    expect(overlay).toBeTruthy();
    // Should contain core shortcuts from shared SHORTCUTS registry
    expect(overlay.textContent).toContain('Ctrl+O');
    expect(overlay.textContent).toContain('F5');
    expect(overlay.textContent).toContain('Ctrl+E');
    expect(overlay.textContent).toContain('?');
  });

  it('clicking close button calls onClose callback', () => {
    const onClose = vi.fn();
    render(() => <KeyboardHelp onClose={onClose} />);
    const closeBtn = screen.getByText('Close');
    fireEvent.click(closeBtn);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('pressing Escape key calls onClose', () => {
    const onClose = vi.fn();
    render(() => <KeyboardHelp onClose={onClose} />);
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('lists correct descriptions for each shortcut', () => {
    render(() => <KeyboardHelp onClose={() => {}} />);
    const overlay = screen.getByTestId('keyboard-help');
    expect(overlay.textContent).toContain('Open file');
    expect(overlay.textContent).toContain('Re-evaluate');
    expect(overlay.textContent).toContain('Export');
    expect(overlay.textContent).toContain('Toggle this help');
  });

  it('renders all active (non-disabled) entries with keys from the shared SHORTCUTS registry', () => {
    render(() => <KeyboardHelp onClose={() => {}} />);
    const overlay = screen.getByTestId('keyboard-help');
    for (const s of SHORTCUTS.filter((s) => s.key && !s.disabled)) {
      expect(overlay.textContent).toContain(s.key);
      expect(overlay.textContent).toContain(s.description);
    }
  });

  it('does NOT render undo shortcut in the overlay', () => {
    render(() => <KeyboardHelp onClose={() => {}} />);
    const overlay = screen.getByTestId('keyboard-help');
    const undo = getShortcut('undo')!;
    expect(overlay.textContent).not.toContain(undo.key);
    expect(overlay.textContent).not.toContain(undo.description);
  });

  it('does NOT render redo shortcut in the overlay', () => {
    render(() => <KeyboardHelp onClose={() => {}} />);
    const overlay = screen.getByTestId('keyboard-help');
    const redo = getShortcut('redo')!;
    expect(overlay.textContent).not.toContain(redo.key);
    expect(overlay.textContent).not.toContain(redo.description);
  });

  it('renders Ctrl+S (save shortcut) from shared registry', () => {
    render(() => <KeyboardHelp onClose={() => {}} />);
    const overlay = screen.getByTestId('keyboard-help');
    expect(overlay.textContent).toContain('Ctrl+S');
  });

  it('renders Ctrl+J (toggle chat shortcut) from shared registry', () => {
    render(() => <KeyboardHelp onClose={() => {}} />);
    const overlay = screen.getByTestId('keyboard-help');
    expect(overlay.textContent).toContain('Ctrl+J');
  });

  it('fold shortcuts surface in the ? overlay (acceptance criterion)', () => {
    render(() => <KeyboardHelp onClose={() => {}} />);
    const overlay = screen.getByTestId('keyboard-help');
    // foldAll / unfoldAll (primary acceptance signal)
    expect(overlay.textContent).toContain('Ctrl+Alt+[');
    expect(overlay.textContent).toContain('Fold all');
    expect(overlay.textContent).toContain('Ctrl+Alt+]');
    expect(overlay.textContent).toContain('Unfold all');
    // fold / unfold at cursor
    expect(overlay.textContent).toContain('Ctrl+Shift+[');
    expect(overlay.textContent).toContain('Fold block at cursor');
    expect(overlay.textContent).toContain('Ctrl+Shift+]');
    expect(overlay.textContent).toContain('Unfold block at cursor');
  });
});
