import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@solidjs/testing-library';
import { MenuBar } from '../panels/MenuBar';

afterEach(() => cleanup());

describe('MenuBar — basic rendering', () => {
  it('renders with data-testid="menu-bar"', () => {
    render(() => <MenuBar />);
    expect(screen.getByTestId('menu-bar')).toBeTruthy();
  });

  it('renders with role="menubar"', () => {
    render(() => <MenuBar />);
    const bar = screen.getByTestId('menu-bar');
    expect(bar.getAttribute('role')).toBe('menubar');
  });

  it('renders four top-level menu triggers: File, Edit, View, Help', () => {
    render(() => <MenuBar />);
    expect(screen.getByText('File')).toBeTruthy();
    expect(screen.getByText('Edit')).toBeTruthy();
    expect(screen.getByText('View')).toBeTruthy();
    expect(screen.getByText('Help')).toBeTruthy();
  });
});

describe('MenuBar — File dropdown', () => {
  it('clicking File trigger opens a dropdown with role="menu"', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    const menu = screen.getByRole('menu');
    expect(menu).toBeTruthy();
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
    expect(screen.getByRole('menu')).toBeTruthy();
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
  it('clicking Open in File menu calls onOpen callback', () => {
    const onOpen = vi.fn();
    render(() => <MenuBar onOpen={onOpen} />);
    fireEvent.click(screen.getByText('File'));
    const items = screen.getAllByRole('menuitem');
    const openItem = items.find((el) => el.textContent?.includes('Open'))!;
    fireEvent.click(openItem);
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it('clicking Save in File menu calls onSave callback', () => {
    const onSave = vi.fn();
    render(() => <MenuBar onSave={onSave} />);
    fireEvent.click(screen.getByText('File'));
    const items = screen.getAllByRole('menuitem');
    const saveItem = items.find((el) => el.textContent?.includes('Save'))!;
    fireEvent.click(saveItem);
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it('clicking Export in File menu calls onExport callback', () => {
    const onExport = vi.fn();
    render(() => <MenuBar onExport={onExport} />);
    fireEvent.click(screen.getByText('File'));
    const items = screen.getAllByRole('menuitem');
    const exportItem = items.find((el) => el.textContent?.includes('Export'))!;
    fireEvent.click(exportItem);
    expect(onExport).toHaveBeenCalledTimes(1);
  });

  it('clicking Re-evaluate in View menu calls onReEvaluate callback', () => {
    const onReEvaluate = vi.fn();
    render(() => <MenuBar onReEvaluate={onReEvaluate} />);
    fireEvent.click(screen.getByText('View'));
    const items = screen.getAllByRole('menuitem');
    const item = items.find((el) => el.textContent?.includes('Re-evaluate'))!;
    fireEvent.click(item);
    expect(onReEvaluate).toHaveBeenCalledTimes(1);
  });

  it('clicking Fit to View in View menu calls onFitToView callback', () => {
    const onFitToView = vi.fn();
    render(() => <MenuBar onFitToView={onFitToView} />);
    fireEvent.click(screen.getByText('View'));
    const items = screen.getAllByRole('menuitem');
    const item = items.find((el) => el.textContent?.includes('Fit to View'))!;
    fireEvent.click(item);
    expect(onFitToView).toHaveBeenCalledTimes(1);
  });

  it('clicking Keyboard Shortcuts in Help menu calls onHelp callback', () => {
    const onHelp = vi.fn();
    render(() => <MenuBar onHelp={onHelp} />);
    fireEvent.click(screen.getByText('Help'));
    const items = screen.getAllByRole('menuitem');
    const item = items.find((el) => el.textContent?.includes('Keyboard Shortcuts'))!;
    fireEvent.click(item);
    expect(onHelp).toHaveBeenCalledTimes(1);
  });

  it('menu closes after clicking a menu item', () => {
    render(() => <MenuBar onOpen={vi.fn()} />);
    fireEvent.click(screen.getByText('File'));
    const items = screen.getAllByRole('menuitem');
    const openItem = items.find((el) => el.textContent?.includes('Open'))!;
    fireEvent.click(openItem);
    expect(screen.queryByRole('menu')).toBeNull();
  });
});

describe('MenuBar — interaction behaviors', () => {
  it('pressing Escape while a menu is open closes it', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByRole('menu')).toBeTruthy();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('hovering another trigger while a menu is open switches to that menu', () => {
    render(() => <MenuBar />);
    fireEvent.click(screen.getByText('File'));
    // File dropdown should be open
    expect(screen.getByRole('menu')).toBeTruthy();
    // Hover Edit trigger
    fireEvent.mouseEnter(screen.getByText('Edit'));
    // Edit dropdown should now be open instead
    const items = screen.getAllByRole('menuitem');
    const labels = items.map((el) => el.textContent ?? '');
    // File-specific items should NOT appear; Edit items should appear
    expect(labels.some((l) => l.includes('Undo'))).toBe(true);
    expect(labels.some((l) => l.includes('Open'))).toBe(false);
  });
});
