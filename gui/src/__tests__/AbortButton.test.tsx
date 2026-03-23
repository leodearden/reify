import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { AbortButton } from '../panels/chat/AbortButton';

describe('AbortButton', () => {
  it('renders with data-testid="abort-button"', () => {
    render(() => <AbortButton onAbort={vi.fn()} />);
    expect(screen.getByTestId('abort-button')).toBeTruthy();
  });

  it('calls onAbort callback on click', () => {
    const onAbort = vi.fn();
    render(() => <AbortButton onAbort={onAbort} />);
    fireEvent.click(screen.getByTestId('abort-button'));
    expect(onAbort).toHaveBeenCalledTimes(1);
  });

  it('has aria-label="Abort generation"', () => {
    render(() => <AbortButton onAbort={vi.fn()} />);
    expect(screen.getByTestId('abort-button').getAttribute('aria-label')).toBe('Abort generation');
  });

  it('contains a stop icon element', () => {
    render(() => <AbortButton onAbort={vi.fn()} />);
    const btn = screen.getByTestId('abort-button');
    // Should contain the ■ stop character
    expect(btn.textContent).toContain('■');
  });
});
