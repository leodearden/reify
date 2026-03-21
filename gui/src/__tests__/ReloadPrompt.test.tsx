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

describe('ReloadPrompt multi-file (R-1)', () => {
  it('renders nothing when filePaths is empty array', () => {
    render(() => (
      <ReloadPrompt filePaths={[]} onReload={vi.fn()} onDismiss={vi.fn()} />
    ));
    expect(screen.queryByTestId('reload-prompt')).toBeNull();
  });

  it('renders single file basename when filePaths has one entry', () => {
    render(() => (
      <ReloadPrompt filePaths={['/project/bracket.ri']} onReload={vi.fn()} onDismiss={vi.fn()} />
    ));
    expect(screen.getByTestId('reload-prompt')).toBeTruthy();
    expect(screen.getByText(/bracket\.ri/)).toBeTruthy();
  });

  it('renders file count when filePaths has multiple entries', () => {
    render(() => (
      <ReloadPrompt
        filePaths={['/project/bracket.ri', '/project/gear.ri']}
        onReload={vi.fn()}
        onDismiss={vi.fn()}
      />
    ));
    expect(screen.getByTestId('reload-prompt')).toBeTruthy();
    expect(screen.getByText(/2 files changed/)).toBeTruthy();
  });

  it('Reload and Dismiss buttons work with filePaths', () => {
    const onReload = vi.fn();
    const onDismiss = vi.fn();
    render(() => (
      <ReloadPrompt
        filePaths={['/project/bracket.ri', '/project/gear.ri']}
        onReload={onReload}
        onDismiss={onDismiss}
      />
    ));
    fireEvent.click(screen.getByText('Reload'));
    expect(onReload).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByText('Dismiss'));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});
