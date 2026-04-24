import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { Toast } from '../panels/Toast';

describe('Toast', () => {
  it('renders with data-testid="toast"', () => {
    render(() => <Toast message="Done" type="success" onDismiss={vi.fn()} />);
    expect(screen.getByTestId('toast')).toBeTruthy();
  });

  it('displays message text from props', () => {
    render(() => <Toast message="Export complete" type="success" onDismiss={vi.fn()} />);
    expect(screen.getByText('Export complete')).toBeTruthy();
  });

  it('applies data-type="success" for success type', () => {
    render(() => <Toast message="OK" type="success" onDismiss={vi.fn()} />);
    expect(screen.getByTestId('toast').dataset.type).toBe('success');
  });

  it('applies data-type="error" for error type', () => {
    render(() => <Toast message="Fail" type="error" onDismiss={vi.fn()} />);
    expect(screen.getByTestId('toast').dataset.type).toBe('error');
  });

  it('applies data-type="info" for info type', () => {
    render(() => <Toast message="Note" type="info" onDismiss={vi.fn()} />);
    expect(screen.getByTestId('toast').dataset.type).toBe('info');
  });

  it('renders a close button', () => {
    render(() => <Toast message="Done" type="success" onDismiss={vi.fn()} />);
    const btn = screen.getByTestId('toast').querySelector('button');
    expect(btn).toBeTruthy();
  });

  it('clicking close button calls onDismiss callback', () => {
    const onDismiss = vi.fn();
    render(() => <Toast message="Done" type="success" onDismiss={onDismiss} />);
    const btn = screen.getByTestId('toast').querySelector('button')!;
    fireEvent.click(btn);
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('close button has aria-label="Close"', () => {
    render(() => <Toast message="Done" type="success" onDismiss={vi.fn()} />);
    const btn = screen.getByTestId('toast').querySelector('button')!;
    expect(btn.getAttribute('aria-label')).toBe('Close');
  });

  it('toast root has role="alert" and aria-live="assertive"', () => {
    render(() => <Toast message="Done" type="success" onDismiss={vi.fn()} />);
    const toast = screen.getByTestId('toast');
    expect(toast.getAttribute('role')).toBe('alert');
    expect(toast.getAttribute('aria-live')).toBe('assertive');
  });

  it('toast element has the animated class for slide-in animation', () => {
    render(() => <Toast message="OK" type="success" onDismiss={vi.fn()} />);
    const toast = screen.getByTestId('toast');
    // CSS module maps class names; check that the animated class is applied
    expect(toast.className).toContain('animated');
  });
});

describe('Toast auto-dismiss', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('calls onDismiss after 3000ms for success type', () => {
    const onDismiss = vi.fn();
    render(() => <Toast message="OK" type="success" onDismiss={onDismiss} />);
    expect(onDismiss).not.toHaveBeenCalled();
    vi.advanceTimersByTime(3000);
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('does not auto-dismiss error toasts (user must close them manually)', () => {
    const onDismiss = vi.fn();
    render(() => <Toast message="Fail" type="error" onDismiss={onDismiss} />);
    vi.advanceTimersByTime(60_000);
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it('calls onDismiss after 3000ms for info type', () => {
    const onDismiss = vi.fn();
    render(() => <Toast message="Note" type="info" onDismiss={onDismiss} />);
    vi.advanceTimersByTime(3000);
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('manual dismiss before timeout does not double-call onDismiss', () => {
    const onDismiss = vi.fn();
    const { unmount } = render(() => <Toast message="OK" type="success" onDismiss={onDismiss} />);
    // Manual dismiss via button click
    const btn = screen.getByTestId('toast').querySelector('button')!;
    fireEvent.click(btn);
    expect(onDismiss).toHaveBeenCalledTimes(1);
    // Unmount to trigger onCleanup (clears the timer)
    unmount();
    // Advance past timeout — should NOT call onDismiss again
    vi.advanceTimersByTime(5000);
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});

// Toast action buttons — step-23 tests (fail until step-24 extends the component)
describe('Toast — action buttons (step-23)', () => {
  it('(a) when actions prop is omitted, no extra action buttons render (only the close button)', () => {
    render(() => <Toast message="Simple" type="info" onDismiss={vi.fn()} />);
    const toast = screen.getByTestId('toast');
    // The only button should be the existing close button (aria-label="Close")
    const buttons = Array.from(toast.querySelectorAll('button'));
    const nonClose = buttons.filter((b) => b.getAttribute('aria-label') !== 'Close');
    expect(nonClose).toHaveLength(0);
  });

  it('(b) when actions provided, renders buttons with the correct labels', () => {
    const yesClick = vi.fn();
    const noClick = vi.fn();
    const ignoreClick = vi.fn();
    render(() => (
      <Toast
        message="Rebind?"
        type="info"
        onDismiss={vi.fn()}
        actions={[
          { label: 'Yes', onClick: yesClick },
          { label: 'No', onClick: noClick },
          { label: 'Ignore', onClick: ignoreClick },
        ]}
      />
    ));
    expect(screen.getByText('Yes')).toBeTruthy();
    expect(screen.getByText('No')).toBeTruthy();
    expect(screen.getByText('Ignore')).toBeTruthy();
  });

  it('(c) clicking an action button calls its onClick AND calls onDismiss', () => {
    const onDismiss = vi.fn();
    const yesClick = vi.fn();
    render(() => (
      <Toast
        message="Rebind?"
        type="info"
        onDismiss={onDismiss}
        actions={[{ label: 'Yes', onClick: yesClick }]}
      />
    ));
    const yesBtn = screen.getByText('Yes') as HTMLElement;
    fireEvent.click(yesBtn);
    expect(yesClick).toHaveBeenCalledTimes(1);
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('(d) action buttons are focusable (rendered as <button> elements)', () => {
    render(() => (
      <Toast
        message="Rebind?"
        type="info"
        onDismiss={vi.fn()}
        actions={[
          { label: 'Yes', onClick: vi.fn() },
          { label: 'No', onClick: vi.fn() },
        ]}
      />
    ));
    const yesEl = screen.getByText('Yes');
    expect(yesEl.tagName.toLowerCase()).toBe('button');
    const noEl = screen.getByText('No');
    expect(noEl.tagName.toLowerCase()).toBe('button');
  });
});
