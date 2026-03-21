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
});
