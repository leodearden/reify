import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { ErrorBoundary } from 'solid-js';

// Component that throws during render
function ThrowingComponent() {
  throw new Error('test render crash');
  return null as never; // unreachable, satisfies TS
}

// Component that renders normally
function NormalComponent() {
  return (<div data-testid="child-ok">All good</div>);
}

// Reusable fallback matching what index.tsx should use
function renderWithBoundary(child: () => any) {
  return render(() => (
    <ErrorBoundary
      fallback={(err: Error) => (
        <div data-testid="error-boundary-fallback">
          <h2>Something went wrong</h2>
          <p>{err.message}</p>
          <button onClick={() => location.reload()}>Reload</button>
        </div>
      )}
    >
      {child()}
    </ErrorBoundary>
  ));
}

describe('ErrorBoundary', () => {
  it('catches render error and shows fallback with error message', () => {
    // Suppress console.error from the boundary
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      renderWithBoundary(() => <ThrowingComponent />);

      const fallback = screen.getByTestId('error-boundary-fallback');
      expect(fallback).toBeTruthy();
      expect(fallback.textContent).toContain('test render crash');
      expect(fallback.textContent).toContain('Something went wrong');
    } finally {
      spy.mockRestore();
    }
  });

  it('renders children normally when no error occurs', () => {
    renderWithBoundary(() => <NormalComponent />);

    expect(screen.getByTestId('child-ok')).toBeTruthy();
    expect(screen.queryByTestId('error-boundary-fallback')).toBeNull();
  });

  it('fallback shows a Reload button', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      renderWithBoundary(() => <ThrowingComponent />);

      const reloadBtn = screen.getByText('Reload');
      expect(reloadBtn).toBeTruthy();
      expect(reloadBtn.tagName.toLowerCase()).toBe('button');
    } finally {
      spy.mockRestore();
    }
  });
});
