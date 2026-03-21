import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { FileBrowser } from '../panels/FileBrowser';
import type { FileData } from '../types';

const testFiles: FileData[] = [
  { path: '/project/src/bracket.ri', content: 'structure Bracket {}' },
  { path: '/project/src/hinge.ri', content: 'structure Hinge {}' },
];

describe('FileBrowser', () => {
  it('renders with data-testid="file-browser"', () => {
    render(() => (
      <FileBrowser files={testFiles} activeFile={null} onFileClick={vi.fn()} />
    ));
    expect(screen.getByTestId('file-browser')).toBeTruthy();
  });

  it('renders a list item for each file in files prop', () => {
    render(() => (
      <FileBrowser files={testFiles} activeFile={null} onFileClick={vi.fn()} />
    ));
    const items = screen.getByTestId('file-browser').querySelectorAll('[data-testid^="file-item-"]');
    expect(items.length).toBe(2);
  });

  it('displays basename of file path (not full path)', () => {
    render(() => (
      <FileBrowser files={testFiles} activeFile={null} onFileClick={vi.fn()} />
    ));
    expect(screen.getByText('bracket.ri')).toBeTruthy();
    expect(screen.getByText('hinge.ri')).toBeTruthy();
  });

  it('the item matching activeFile prop has data-active="true" attribute', () => {
    render(() => (
      <FileBrowser
        files={testFiles}
        activeFile="/project/src/bracket.ri"
        onFileClick={vi.fn()}
      />
    ));
    const activeItem = screen.getByTestId('file-item-/project/src/bracket.ri');
    expect(activeItem.dataset.active).toBe('true');

    const inactiveItem = screen.getByTestId('file-item-/project/src/hinge.ri');
    expect(inactiveItem.dataset.active).toBeUndefined();
  });

  it('clicking a file item calls onFileClick(path)', () => {
    const onFileClick = vi.fn();
    render(() => (
      <FileBrowser files={testFiles} activeFile={null} onFileClick={onFileClick} />
    ));
    fireEvent.click(screen.getByText('bracket.ri'));
    expect(onFileClick).toHaveBeenCalledWith('/project/src/bracket.ri');
  });

  it('shows "No files" empty state when files array is empty', () => {
    render(() => (
      <FileBrowser files={[]} activeFile={null} onFileClick={vi.fn()} />
    ));
    expect(screen.getByText('No files')).toBeTruthy();
  });
});
