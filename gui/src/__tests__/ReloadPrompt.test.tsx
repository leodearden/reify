import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { ReloadPrompt } from '../panels/ReloadPrompt';

describe('ReloadPrompt', () => {
  it('renders banner with data-testid="reload-prompt" when filePath is non-null', () => {
    render(() => (
      <ReloadPrompt filePath="/project/bracket.ri" onReload={vi.fn()} onDismiss={vi.fn()} />
    ));
    expect(screen.getByTestId('reload-prompt')).toBeTruthy();
  });

  it('is not in DOM when filePath is null', () => {
    render(() => (
      <ReloadPrompt filePath={null} onReload={vi.fn()} onDismiss={vi.fn()} />
    ));
    expect(screen.queryByTestId('reload-prompt')).toBeNull();
  });

  it('displays the basename of filePath in the message', () => {
    render(() => (
      <ReloadPrompt filePath="/project/src/bracket.ri" onReload={vi.fn()} onDismiss={vi.fn()} />
    ));
    expect(screen.getByText(/bracket\.ri/)).toBeTruthy();
  });

  it('clicking Reload button calls onReload', () => {
    const onReload = vi.fn();
    render(() => (
      <ReloadPrompt filePath="/project/bracket.ri" onReload={onReload} onDismiss={vi.fn()} />
    ));
    fireEvent.click(screen.getByText('Reload'));
    expect(onReload).toHaveBeenCalledTimes(1);
  });

  it('clicking Dismiss button calls onDismiss', () => {
    const onDismiss = vi.fn();
    render(() => (
      <ReloadPrompt filePath="/project/bracket.ri" onReload={vi.fn()} onDismiss={onDismiss} />
    ));
    fireEvent.click(screen.getByText('Dismiss'));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});
