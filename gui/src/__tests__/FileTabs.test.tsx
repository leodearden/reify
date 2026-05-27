import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { createRoot } from 'solid-js';
import { FileTabs } from '../editor/FileTabs';
import { createEditorStore } from '../stores/editorStore';
import type { FileData } from '../types';

const file1: FileData = { path: '/project/src/bracket.ri', content: 'structure Bracket {}' };
const file2: FileData = { path: '/project/src/mount.ri', content: 'structure Mount {}' };

function setup(files: FileData[] = [file1, file2], opts?: { dirty?: string[] }) {
  const store = createEditorStore();
  for (const f of files) store.openFile(f);
  if (opts?.dirty) {
    for (const p of opts.dirty) store.markDirty(p);
  }
  return store;
}

describe('FileTabs', () => {
  it('renders a tab for each open file', () => {
    const store = setup();
    render(() => <FileTabs store={store} />);
    const tabs = screen.getAllByTestId('file-tab');
    expect(tabs).toHaveLength(2);
  });

  it('shows basename not full path', () => {
    const store = setup();
    render(() => <FileTabs store={store} />);
    expect(screen.getByText('bracket.ri')).toBeTruthy();
    expect(screen.getByText('mount.ri')).toBeTruthy();
  });

  it('active tab has aria-selected=true', () => {
    const store = setup();
    // After opening both, activeFile is the last opened (mount.ri)
    render(() => <FileTabs store={store} />);
    const tabs = screen.getAllByTestId('file-tab');
    const mountTab = tabs.find((t) => t.textContent?.includes('mount.ri'));
    const bracketTab = tabs.find((t) => t.textContent?.includes('bracket.ri'));
    expect(mountTab?.getAttribute('aria-selected')).toBe('true');
    expect(bracketTab?.getAttribute('aria-selected')).toBe('false');
  });

  it('dirty files show a dirty indicator', () => {
    const store = setup([file1, file2], { dirty: [file1.path] });
    render(() => <FileTabs store={store} />);
    const indicators = screen.getAllByTestId('dirty-indicator');
    expect(indicators).toHaveLength(1);
  });

  it('each tab has a close button', () => {
    const store = setup();
    render(() => <FileTabs store={store} />);
    const closeBtns = screen.getAllByTestId('close-tab');
    expect(closeBtns).toHaveLength(2);
  });

  it('clicking a tab calls setActiveFile', () => {
    const store = setup();
    const spy = vi.spyOn(store, 'setActiveFile');
    render(() => <FileTabs store={store} />);
    const tabs = screen.getAllByTestId('file-tab');
    const bracketTab = tabs.find((t) => t.textContent?.includes('bracket.ri'))!;
    fireEvent.click(bracketTab);
    expect(spy).toHaveBeenCalledWith(file1.path);
  });

  it('clicking close calls closeFile without switching tab', () => {
    const store = setup();
    const closeSpy = vi.spyOn(store, 'closeFile');
    const activeSpy = vi.spyOn(store, 'setActiveFile');
    render(() => <FileTabs store={store} />);
    // Close the bracket tab (not the active one)
    const closeBtns = screen.getAllByTestId('close-tab');
    // bracket.ri is rendered first
    fireEvent.click(closeBtns[0]);
    expect(closeSpy).toHaveBeenCalledWith(file1.path);
    // setActiveFile should NOT be called by the close button click itself
    expect(activeSpy).not.toHaveBeenCalled();
  });

  describe('tablist semantics and keyboard navigation', () => {
    it('tab bar container has role=tablist', () => {
      const store = setup();
      render(() => <FileTabs store={store} />);
      const tabBar = screen.getByTestId('file-tabs');
      expect(tabBar.getAttribute('role')).toBe('tablist');
    });

    it('active tab has tabindex=0, inactive tabs have tabindex=-1', () => {
      const store = setup();
      render(() => <FileTabs store={store} />);
      const tabs = screen.getAllByTestId('file-tab');
      const mountTab = tabs.find((t) => t.textContent?.includes('mount.ri'))!;
      const bracketTab = tabs.find((t) => t.textContent?.includes('bracket.ri'))!;
      // mount.ri is the active tab (last opened)
      expect(mountTab.getAttribute('tabindex')).toBe('0');
      expect(bracketTab.getAttribute('tabindex')).toBe('-1');
    });

    it('each tab has a title attribute with full file path', () => {
      const store = setup();
      render(() => <FileTabs store={store} />);
      const tabs = screen.getAllByTestId('file-tab');
      const bracketTab = tabs.find((t) => t.textContent?.includes('bracket.ri'))!;
      const mountTab = tabs.find((t) => t.textContent?.includes('mount.ri'))!;
      expect(bracketTab.getAttribute('title')).toBe(file1.path);
      expect(mountTab.getAttribute('title')).toBe(file2.path);
    });

    it('ArrowRight on active tab activates the next tab', () => {
      const store = setup();
      const spy = vi.spyOn(store, 'setActiveFile');
      render(() => <FileTabs store={store} />);
      // bracket.ri is first, mount.ri is active (last opened)
      // Make bracket.ri active first
      store.setActiveFile(file1.path);
      spy.mockClear();
      const tabs = screen.getAllByTestId('file-tab');
      const bracketTab = tabs.find((t) => t.textContent?.includes('bracket.ri'))!;
      fireEvent.keyDown(bracketTab, { key: 'ArrowRight' });
      expect(spy).toHaveBeenCalledWith(file2.path);
    });

    it('ArrowLeft on active tab activates the previous tab', () => {
      const store = setup();
      const spy = vi.spyOn(store, 'setActiveFile');
      render(() => <FileTabs store={store} />);
      // mount.ri is active (last opened, index 1)
      const tabs = screen.getAllByTestId('file-tab');
      const mountTab = tabs.find((t) => t.textContent?.includes('mount.ri'))!;
      fireEvent.keyDown(mountTab, { key: 'ArrowLeft' });
      expect(spy).toHaveBeenCalledWith(file1.path);
    });

    it('ArrowRight on last tab wraps to first', () => {
      const store = setup();
      const spy = vi.spyOn(store, 'setActiveFile');
      render(() => <FileTabs store={store} />);
      // mount.ri is active (last opened, index 1 which is the last tab)
      const tabs = screen.getAllByTestId('file-tab');
      const mountTab = tabs.find((t) => t.textContent?.includes('mount.ri'))!;
      fireEvent.keyDown(mountTab, { key: 'ArrowRight' });
      expect(spy).toHaveBeenCalledWith(file1.path);
    });

    it('ArrowLeft on first tab wraps to last', () => {
      const store = setup();
      const spy = vi.spyOn(store, 'setActiveFile');
      render(() => <FileTabs store={store} />);
      store.setActiveFile(file1.path);
      spy.mockClear();
      const tabs = screen.getAllByTestId('file-tab');
      const bracketTab = tabs.find((t) => t.textContent?.includes('bracket.ri'))!;
      fireEvent.keyDown(bracketTab, { key: 'ArrowLeft' });
      expect(spy).toHaveBeenCalledWith(file2.path);
    });
  });

  describe('FileTabs externally-changed indicator', () => {
    it('(a) tab with path in externallyChanged renders externally-changed-indicator', () => {
      const store = setup([file1, file2]);
      store.markExternallyChanged(file1.path);
      render(() => <FileTabs store={store} />);
      const indicators = screen.getAllByTestId('externally-changed-indicator');
      expect(indicators).toHaveLength(1);
    });

    it('(b) tabs NOT in externallyChanged do NOT render the indicator', () => {
      const store = setup([file1, file2]);
      // Neither file is externally changed
      render(() => <FileTabs store={store} />);
      const indicators = screen.queryAllByTestId('externally-changed-indicator');
      expect(indicators).toHaveLength(0);
    });

    it('(c) a tab that is BOTH dirty AND externally changed renders both indicators', () => {
      const store = setup([file1, file2]);
      store.markDirty(file1.path);
      store.markExternallyChanged(file1.path);
      render(() => <FileTabs store={store} />);
      // The tab for file1 should have both indicators
      const dirtyIndicators = screen.getAllByTestId('dirty-indicator');
      const externallyChangedIndicators = screen.getAllByTestId('externally-changed-indicator');
      expect(dirtyIndicators).toHaveLength(1);
      expect(externallyChangedIndicators).toHaveLength(1);
      // Confirm they are distinct elements (different data-testid)
      expect(dirtyIndicators[0]).not.toBe(externallyChangedIndicators[0]);
    });
  });

  describe('unsaved changes confirmation', () => {
    it('clicking close on a dirty tab calls window.confirm with filename', () => {
      const store = setup([file1, file2], { dirty: [file1.path] });
      const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(false);
      render(() => <FileTabs store={store} />);
      const closeBtns = screen.getAllByTestId('close-tab');
      fireEvent.click(closeBtns[0]); // bracket.ri is dirty
      expect(confirmSpy).toHaveBeenCalledTimes(1);
      expect(confirmSpy.mock.calls[0][0]).toContain('bracket.ri');
      confirmSpy.mockRestore();
    });

    it('cancelling confirm does NOT call closeFile', () => {
      const store = setup([file1, file2], { dirty: [file1.path] });
      const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(false);
      const closeSpy = vi.spyOn(store, 'closeFile');
      render(() => <FileTabs store={store} />);
      const closeBtns = screen.getAllByTestId('close-tab');
      fireEvent.click(closeBtns[0]);
      expect(closeSpy).not.toHaveBeenCalled();
      confirmSpy.mockRestore();
    });

    it('confirming close on dirty tab calls closeFile', () => {
      const store = setup([file1, file2], { dirty: [file1.path] });
      const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true);
      const closeSpy = vi.spyOn(store, 'closeFile');
      render(() => <FileTabs store={store} />);
      const closeBtns = screen.getAllByTestId('close-tab');
      fireEvent.click(closeBtns[0]);
      expect(closeSpy).toHaveBeenCalledWith(file1.path);
      confirmSpy.mockRestore();
    });

    it('clicking close on a clean tab does NOT call window.confirm', () => {
      const store = setup([file1, file2]); // no dirty files
      const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(false);
      const closeSpy = vi.spyOn(store, 'closeFile');
      render(() => <FileTabs store={store} />);
      const closeBtns = screen.getAllByTestId('close-tab');
      fireEvent.click(closeBtns[0]);
      expect(confirmSpy).not.toHaveBeenCalled();
      expect(closeSpy).toHaveBeenCalledWith(file1.path);
      confirmSpy.mockRestore();
    });
  });
});
