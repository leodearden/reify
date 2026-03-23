import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { ContextChip } from '../panels/chat/ContextChip';

describe('ContextChip', () => {
  it('renders with data-testid="context-chip"', () => {
    render(() => <ContextChip label="Selection" type="selection" onRemove={() => {}} />);
    expect(screen.getByTestId('context-chip')).toBeTruthy();
  });

  it('displays label text prop', () => {
    render(() => <ContextChip label="Active diagnostics" type="diagnostics" onRemove={() => {}} />);
    expect(screen.getByText('Active diagnostics')).toBeTruthy();
  });

  it('shows remove button with data-testid="chip-remove"', () => {
    render(() => <ContextChip label="Constraints" type="constraints" onRemove={() => {}} />);
    expect(screen.getByTestId('chip-remove')).toBeTruthy();
  });

  it('clicking remove calls onRemove callback', () => {
    const onRemove = vi.fn();
    render(() => <ContextChip label="Selection" type="selection" onRemove={onRemove} />);
    fireEvent.click(screen.getByTestId('chip-remove'));
    expect(onRemove).toHaveBeenCalledOnce();
  });

  it('different context types set data-context-type attribute', () => {
    const types = ['selection', 'diagnostics', 'constraints', 'file'] as const;
    for (const type of types) {
      const { unmount } = render(() => (
        <ContextChip label={`Label ${type}`} type={type} onRemove={() => {}} />
      ));
      const el = screen.getByTestId('context-chip');
      expect(el.getAttribute('data-context-type')).toBe(type);
      unmount();
    }
  });

  it('chip is keyboard-accessible (Enter on remove button triggers callback)', () => {
    const onRemove = vi.fn();
    render(() => <ContextChip label="Selection" type="selection" onRemove={onRemove} />);
    const btn = screen.getByTestId('chip-remove');
    fireEvent.keyDown(btn, { key: 'Enter' });
    expect(onRemove).toHaveBeenCalledOnce();
  });
});
