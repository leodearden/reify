import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@solidjs/testing-library';
import { MenuBar } from '../panels/MenuBar';
import { getShortcut } from '../shortcuts';

afterEach(() => cleanup());

describe('MenuBar — basic rendering', () => {
  it('renders with data-testid="menu-bar"', () => {
    render(() => <MenuBar />);
    expect(screen.getByTestId('menu-bar')).not.toBeNull();
  });

  it('renders with role="menubar"', () => {
    render(() => <MenuBar />);
    const bar = screen.getByTestId('menu-bar');
    expect(bar.getAttribute('role')).toBe('menubar');
  });

  it('renders four top-level menu triggers: File, Edit, View, Help', () => {
    render(() => <MenuBar />);
    expect(screen.getByText('File')).not.toBeNull();
    expect(screen.getByText('Edit')).not.toBeNull();
    expect(screen.getByText('View')).not.toBeNull();
    expect(screen.getByText('Help')).not.toBeNull();
  });
});

describe('MenuBar — File dropdown', () => {
  it('clicking File trigger opens a dropdown with role="menu"', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    const menu = screen.getByRole('menu');
    expect(menu).not.toBeNull();
  });

  it('File dropdown contains Open, Save, Export items', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    const items = screen.getAllByRole('menuitem');
    const labels = items.map((el) => el.textContent ?? '');
    expect(labels.some((l) => l.includes('Open'))).toBe(true);
    expect(labels.some((l) => l.includes('Save'))).toBe(true);
    expect(labels.some((l) => l.includes('Export'))).toBe(true);
  });

  it('Open item shows shortcut annotation Ctrl+O', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    const items = screen.getAllByRole('menuitem');
    const openItem = items.find((el) => el.textContent?.includes('Open'));
    expect(openItem?.textContent).toContain('Ctrl+O');
  });

  it('Save item shows shortcut annotation Ctrl+S', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    const items = screen.getAllByRole('menuitem');
    const saveItem = items.find((el) => el.textContent?.includes('Save'));
    expect(saveItem?.textContent).toContain('Ctrl+S');
  });

  it('Export item shows shortcut annotation Ctrl+E', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    const items = screen.getAllByRole('menuitem');
    const exportItem = items.find((el) => el.textContent?.includes('Export'));
    expect(exportItem?.textContent).toContain('Ctrl+E');
  });

  it('clicking File trigger again closes the dropdown', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByRole('menu')).not.toBeNull();
    fireEvent.click(screen.getByText('File'));
    expect(screen.queryByRole('menu')).toBeNull();
  });
});

describe('MenuBar — View dropdown', () => {
  it('clicking View opens a dropdown containing Re-evaluate, Fit to View, Toggle Chat', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('View'));
    const items = screen.getAllByRole('menuitem');
    const labels = items.map((el) => el.textContent ?? '');
    expect(labels.some((l) => l.includes('Re-evaluate'))).toBe(true);
    expect(labels.some((l) => l.includes('Fit to View'))).toBe(true);
    expect(labels.some((l) => l.includes('Toggle Chat'))).toBe(true);
  });

  it('Re-evaluate item shows F5 annotation', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('View'));
    const items = screen.getAllByRole('menuitem');
    const item = items.find((el) => el.textContent?.includes('Re-evaluate'));
    expect(item?.textContent).toContain('F5');
  });

  it('Toggle Chat item shows Ctrl+J annotation', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('View'));
    const items = screen.getAllByRole('menuitem');
    const item = items.find((el) => el.textContent?.includes('Toggle Chat'));
    expect(item?.textContent).toContain('Ctrl+J');
  });
});

describe('MenuBar — Help dropdown', () => {
  it('clicking Help opens a dropdown with Keyboard Shortcuts item', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('Help'));
    const items = screen.getAllByRole('menuitem');
    const labels = items.map((el) => el.textContent ?? '');
    expect(labels.some((l) => l.includes('Keyboard Shortcuts'))).toBe(true);
  });

  it('Keyboard Shortcuts item shows ? annotation', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('Help'));
    const items = screen.getAllByRole('menuitem');
    const item = items.find((el) => el.textContent?.includes('Keyboard Shortcuts'));
    expect(item?.textContent).toContain('?');
  });
});

describe('MenuBar — callbacks', () => {
  function clickMenuItem(label: string) {
    const item = screen.getAllByRole('menuitem').find((el) => el.textContent?.includes(label));
    expect(item).toBeTruthy();
    fireEvent.click(item!);
  }

  it('clicking Open in File menu calls onOpen callback', () => {
    const onOpen = vi.fn();
    render(() => <MenuBar onOpen={onOpen} />);
    fireEvent.click(screen.getByText('File'));
    clickMenuItem('Open');
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it('clicking Save in File menu calls onSave callback', () => {
    const onSave = vi.fn();
    render(() => <MenuBar onSave={onSave} />);
    fireEvent.click(screen.getByText('File'));
    clickMenuItem('Save');
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it('clicking Export in File menu calls onExport callback', () => {
    const onExport = vi.fn();
    render(() => <MenuBar onExport={onExport} />);
    fireEvent.click(screen.getByText('File'));
    clickMenuItem('Export');
    expect(onExport).toHaveBeenCalledTimes(1);
  });

  it('clicking Re-evaluate in View menu calls onReEvaluate callback', () => {
    const onReEvaluate = vi.fn();
    render(() => <MenuBar onReEvaluate={onReEvaluate} />);
    fireEvent.click(screen.getByText('View'));
    clickMenuItem('Re-evaluate');
    expect(onReEvaluate).toHaveBeenCalledTimes(1);
  });

  it('clicking Fit to View in View menu calls onFitToView callback', () => {
    const onFitToView = vi.fn();
    render(() => <MenuBar onFitToView={onFitToView} />);
    fireEvent.click(screen.getByText('View'));
    clickMenuItem('Fit to View');
    expect(onFitToView).toHaveBeenCalledTimes(1);
  });

  it('clicking Keyboard Shortcuts in Help menu calls onHelp callback', () => {
    const onHelp = vi.fn();
    render(() => <MenuBar onHelp={onHelp} />);
    fireEvent.click(screen.getByText('Help'));
    clickMenuItem('Keyboard Shortcuts');
    expect(onHelp).toHaveBeenCalledTimes(1);
  });

  it('menu closes after clicking a menu item', () => {
    render(() => <MenuBar onOpen={vi.fn()} />);
    fireEvent.click(screen.getByText('File'));
    clickMenuItem('Open');
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('clicking Toggle Chat in View menu calls onToggleChat callback', () => {
    const onToggleChat = vi.fn();
    render(() => <MenuBar onToggleChat={onToggleChat} />);
    fireEvent.click(screen.getByText('View'));
    clickMenuItem('Toggle Chat');
    expect(onToggleChat).toHaveBeenCalledTimes(1);
  });
});

describe('MenuBar — Edit dropdown', () => {
  it('Edit dropdown contains Undo and Redo items, both disabled', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('Edit'));
    const items = screen.getAllByRole('menuitem');
    const undoItem = items.find((el) => el.textContent?.includes('Undo'));
    const redoItem = items.find((el) => el.textContent?.includes('Redo'));
    expect(undoItem).toBeDefined();
    expect(redoItem).toBeDefined();
    expect((undoItem as HTMLButtonElement).disabled).toBe(true);
    expect((redoItem as HTMLButtonElement).disabled).toBe(true);
  });

});

describe('MenuBar — disabled derivation', () => {
  it('Non-disabled shortcut items (File menu) render as enabled per registry', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    const items = screen.getAllByRole('menuitem');
    const openItem = items.find((el) => el.textContent?.includes('Open')) as HTMLButtonElement;
    const saveItem = items.find((el) => el.textContent?.includes('Save')) as HTMLButtonElement;
    const exportItem = items.find((el) => el.textContent?.includes('Export')) as HTMLButtonElement;
    expect(openItem).toBeDefined();
    expect(saveItem).toBeDefined();
    expect(exportItem).toBeDefined();
    expect(openItem.disabled).toBe(getShortcut('open')?.disabled ?? false);
    expect(saveItem.disabled).toBe(getShortcut('save')?.disabled ?? false);
    expect(exportItem.disabled).toBe(getShortcut('export')?.disabled ?? false);
    // Confirm these evaluate to false (registry has no disabled flag for these)
    expect(openItem.disabled).toBe(false);
    expect(saveItem.disabled).toBe(false);
    expect(exportItem.disabled).toBe(false);
  });
});

describe('MenuBar — interaction behaviors', () => {
  it('pressing Escape while a menu is open closes it', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByRole('menu')).not.toBeNull();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('hovering another trigger while a menu is open switches to that menu', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    // File dropdown should be open
    expect(screen.getByRole('menu')).not.toBeNull();
    // Hover Edit trigger
    fireEvent.mouseEnter(screen.getByText('Edit'));
    // Edit dropdown should now be open instead
    const items = screen.getAllByRole('menuitem');
    const labels = items.map((el) => el.textContent ?? '');
    // File-specific items should NOT appear; Edit items should appear
    expect(labels.some((l) => l.includes('Undo'))).toBe(true);
    expect(labels.some((l) => l.includes('Open'))).toBe(false);
  });

  it('does not close the menu when mousedown target is not a Node', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByRole('menu')).not.toBeNull();
    // Dispatch a mousedown where target is a non-Node (e.g. null via Object.create)
    const event = new MouseEvent('mousedown', { bubbles: true, cancelable: true });
    Object.defineProperty(event, 'target', { value: null, writable: false });
    document.dispatchEvent(event);
    // Menu should remain open (guard skips when target is not a Node)
    expect(screen.getByRole('menu')).not.toBeNull();
  });
});

// ---------------------------------------------------------------------------
// File→New item (task-3209)
// ---------------------------------------------------------------------------

describe('MenuBar — File→New item', () => {
  function clickMenuItem(label: string) {
    const item = screen.getAllByRole('menuitem').find((el) => el.textContent?.includes(label));
    expect(item).toBeTruthy();
    fireEvent.click(item!);
  }

  it('File dropdown contains a New item', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    const items = screen.getAllByRole('menuitem');
    const labels = items.map((el) => el.textContent ?? '');
    expect(labels.some((l) => l.includes('New'))).toBe(true);
  });

  it('New is the first item in the File dropdown, above Open', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    const items = screen.getAllByRole('menuitem');
    const labels = items.map((el) => el.textContent ?? '');
    const newIdx = labels.findIndex((l) => l.includes('New'));
    const openIdx = labels.findIndex((l) => l.includes('Open'));
    expect(newIdx).toBeGreaterThanOrEqual(0);
    expect(newIdx).toBeLessThan(openIdx);
  });

  it('New item shows Ctrl+N shortcut annotation', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    const items = screen.getAllByRole('menuitem');
    const newItem = items.find((el) => el.textContent?.includes('New'));
    expect(newItem?.textContent).toContain('Ctrl+N');
  });

  it('clicking New calls onNew callback', () => {
    const onNew = vi.fn();
    render(() => <MenuBar onNew={onNew} />);
    fireEvent.click(screen.getByText('File'));
    clickMenuItem('New');
    expect(onNew).toHaveBeenCalledTimes(1);
  });

  it('menu closes after clicking New', () => {
    render(() => <MenuBar onNew={vi.fn()} />);
    fireEvent.click(screen.getByText('File'));
    clickMenuItem('New');
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('New item is enabled (not disabled) per registry derivation', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    const items = screen.getAllByRole('menuitem');
    const newItem = items.find((el) => el.textContent?.includes('New')) as HTMLButtonElement;
    expect(newItem).toBeDefined();
    expect(newItem.disabled).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// task-4295: menu-button testids
// ---------------------------------------------------------------------------

describe('MenuBar — trigger data-testid', () => {
  it('File trigger has data-testid="menu-trigger-file"', () => {
    render(() => <MenuBar />);
    expect(screen.getByTestId('menu-trigger-file')).not.toBeNull();
  });

  it('Edit trigger has data-testid="menu-trigger-edit"', () => {
    render(() => <MenuBar />);
    expect(screen.getByTestId('menu-trigger-edit')).not.toBeNull();
  });

  it('View trigger has data-testid="menu-trigger-view"', () => {
    render(() => <MenuBar />);
    expect(screen.getByTestId('menu-trigger-view')).not.toBeNull();
  });

  it('Help trigger has data-testid="menu-trigger-help"', () => {
    render(() => <MenuBar />);
    expect(screen.getByTestId('menu-trigger-help')).not.toBeNull();
  });

  it('File items have data-testid="menu-item-new/open/save/export"', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByTestId('menu-trigger-file'));
    expect(screen.getByTestId('menu-item-new')).not.toBeNull();
    expect(screen.getByTestId('menu-item-open')).not.toBeNull();
    expect(screen.getByTestId('menu-item-save')).not.toBeNull();
    expect(screen.getByTestId('menu-item-export')).not.toBeNull();
  });

  it('View items have data-testid="menu-item-reEvaluate/fitToView/toggleChat"', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByTestId('menu-trigger-view'));
    expect(screen.getByTestId('menu-item-reEvaluate')).not.toBeNull();
    expect(screen.getByTestId('menu-item-fitToView')).not.toBeNull();
    expect(screen.getByTestId('menu-item-toggleChat')).not.toBeNull();
  });

  it('Help items have data-testid="menu-item-help"', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByTestId('menu-trigger-help'));
    expect(screen.getByTestId('menu-item-help')).not.toBeNull();
  });

  it('Edit items have data-testid="menu-item-undo/redo"', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByTestId('menu-trigger-edit'));
    expect(screen.getByTestId('menu-item-undo')).not.toBeNull();
    expect(screen.getByTestId('menu-item-redo')).not.toBeNull();
  });
});
