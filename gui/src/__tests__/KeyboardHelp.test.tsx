import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { KeyboardHelp } from '../components/KeyboardHelp';
import { SHORTCUTS } from '../shortcuts';

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

  it('does NOT render Ctrl+Z (undo) in the overlay', () => {
    render(() => <KeyboardHelp onClose={() => {}} />);
    const overlay = screen.getByTestId('keyboard-help');
    expect(overlay.textContent).not.toContain('Ctrl+Z');
    expect(overlay.textContent).not.toContain('Undo');
  });

  it('does NOT render Ctrl+Shift+Z (redo) in the overlay', () => {
    render(() => <KeyboardHelp onClose={() => {}} />);
    const overlay = screen.getByTestId('keyboard-help');
    expect(overlay.textContent).not.toContain('Ctrl+Shift+Z');
    expect(overlay.textContent).not.toContain('Redo');
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
});
