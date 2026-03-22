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

  describe('ARIA semantics and keyboard accessibility', () => {
    it('container has role=listbox', () => {
      render(() => (
        <FileBrowser files={testFiles} activeFile={null} onFileClick={vi.fn()} />
      ));
      expect(screen.getByTestId('file-browser').getAttribute('role')).toBe('listbox');
    });

    it('container has aria-label=File browser', () => {
      render(() => (
        <FileBrowser files={testFiles} activeFile={null} onFileClick={vi.fn()} />
      ));
      expect(screen.getByTestId('file-browser').getAttribute('aria-label')).toBe('File browser');
    });

    it('each file item has role=option', () => {
      render(() => (
        <FileBrowser files={testFiles} activeFile={null} onFileClick={vi.fn()} />
      ));
      const items = screen.getByTestId('file-browser').querySelectorAll('[role="option"]');
      expect(items.length).toBe(2);
    });

    it('active item has aria-selected=true, non-active has aria-selected=false', () => {
      render(() => (
        <FileBrowser
          files={testFiles}
          activeFile="/project/src/bracket.ri"
          onFileClick={vi.fn()}
        />
      ));
      const activeItem = screen.getByTestId('file-item-/project/src/bracket.ri');
      const inactiveItem = screen.getByTestId('file-item-/project/src/hinge.ri');
      expect(activeItem.getAttribute('aria-selected')).toBe('true');
      expect(inactiveItem.getAttribute('aria-selected')).toBe('false');
    });

    it('each item has title attribute with full file path', () => {
      render(() => (
        <FileBrowser files={testFiles} activeFile={null} onFileClick={vi.fn()} />
      ));
      const bracketItem = screen.getByTestId('file-item-/project/src/bracket.ri');
      const hingeItem = screen.getByTestId('file-item-/project/src/hinge.ri');
      expect(bracketItem.getAttribute('title')).toBe('/project/src/bracket.ri');
      expect(hingeItem.getAttribute('title')).toBe('/project/src/hinge.ri');
    });

    it('active item has tabindex=0, others have tabindex=-1', () => {
      render(() => (
        <FileBrowser
          files={testFiles}
          activeFile="/project/src/bracket.ri"
          onFileClick={vi.fn()}
        />
      ));
      const activeItem = screen.getByTestId('file-item-/project/src/bracket.ri');
      const inactiveItem = screen.getByTestId('file-item-/project/src/hinge.ri');
      expect(activeItem.getAttribute('tabindex')).toBe('0');
      expect(inactiveItem.getAttribute('tabindex')).toBe('-1');
    });

    it('ArrowDown on focused item moves focus/selection to next item', () => {
      const onFileClick = vi.fn();
      render(() => (
        <FileBrowser
          files={testFiles}
          activeFile="/project/src/bracket.ri"
          onFileClick={onFileClick}
        />
      ));
      const bracketItem = screen.getByTestId('file-item-/project/src/bracket.ri');
      fireEvent.keyDown(bracketItem, { key: 'ArrowDown' });
      expect(onFileClick).toHaveBeenCalledWith('/project/src/hinge.ri');
    });

    it('ArrowUp on focused item moves to previous item', () => {
      const onFileClick = vi.fn();
      render(() => (
        <FileBrowser
          files={testFiles}
          activeFile="/project/src/hinge.ri"
          onFileClick={onFileClick}
        />
      ));
      const hingeItem = screen.getByTestId('file-item-/project/src/hinge.ri');
      fireEvent.keyDown(hingeItem, { key: 'ArrowUp' });
      expect(onFileClick).toHaveBeenCalledWith('/project/src/bracket.ri');
    });

    it('active item has the activeItem CSS class for enhanced indicator', () => {
      render(() => (
        <FileBrowser
          files={testFiles}
          activeFile="/project/src/bracket.ri"
          onFileClick={vi.fn()}
        />
      ));
      const activeItem = screen.getByTestId('file-item-/project/src/bracket.ri');
      const inactiveItem = screen.getByTestId('file-item-/project/src/hinge.ri');
      // Active item should have the activeItem CSS module class
      expect(activeItem.className).toContain('activeItem');
      expect(inactiveItem.className).not.toContain('activeItem');
    });

    it('Enter on focused item calls onFileClick with that path', () => {
      const onFileClick = vi.fn();
      render(() => (
        <FileBrowser
          files={testFiles}
          activeFile="/project/src/bracket.ri"
          onFileClick={onFileClick}
        />
      ));
      const bracketItem = screen.getByTestId('file-item-/project/src/bracket.ri');
      fireEvent.keyDown(bracketItem, { key: 'Enter' });
      expect(onFileClick).toHaveBeenCalledWith('/project/src/bracket.ri');
    });
  });
});
