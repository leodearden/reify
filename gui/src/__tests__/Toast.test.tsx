import { describe, it, expect, vi } from 'vitest';
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
});
