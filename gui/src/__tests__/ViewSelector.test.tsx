/**
 * Tests for <ViewSelector> component (step-21).
 *
 * ViewSelector renders a dropdown trigger showing the active view name.
 * Clicking opens a dropdown listing auto views first, then user views in
 * userViewOrder.  A footer row "Organize views…" fires onOpenManage.
 * Escape and click-outside close the dropdown.  Modified user views show
 * a visual marker.
 */
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@solidjs/testing-library';
import { createRoot } from 'solid-js';
import { ViewSelector } from '../panels/ViewSelector';
import { createViewStateStore } from '../stores/viewStateStore';
import { makeNode } from './test-utils';

function makeTree() {
  return [
    makeNode({
      entity_path: 'Root',
      kind: 'structure',
      children: [
        makeNode({ entity_path: 'Root.A', kind: 'param', trait_geometry: true }),
      ],
    }),
  ];
}

function makeStore() {
  let store: ReturnType<typeof createViewStateStore>;
  createRoot(() => {
    store = createViewStateStore();
    store.regenerateAutoViews(makeTree());
  });
  return store!;
}

describe('ViewSelector — trigger button', () => {
  it('renders a button labelled with the active view name', () => {
    const store = makeStore();
    render(() => <ViewSelector store={store} onOpenManage={vi.fn()} />);
    // Default active view is "auto:default" → name "Default"
    const trigger = screen.getByRole('button', { name: /default/i });
    expect(trigger).toBeTruthy();
  });

  it('trigger label updates when activeViewId changes', () => {
    const store = makeStore();
    // Create a user view and switch to it
    let uid!: string;
    createRoot(() => {
      uid = store.createView('My Custom View');
      store.switchView(uid);
    });
    render(() => <ViewSelector store={store} onOpenManage={vi.fn()} />);
    expect(screen.getByRole('button', { name: /my custom view/i })).toBeTruthy();
  });
});

describe('ViewSelector — dropdown open/close', () => {
  it('clicking the trigger opens the dropdown', () => {
    const store = makeStore();
    render(() => <ViewSelector store={store} onOpenManage={vi.fn()} />);
    const trigger = screen.getByRole('button', { name: /default/i });
    fireEvent.click(trigger);
    // Dropdown should appear
    expect(screen.getByRole('menu')).toBeTruthy();
  });

  it('dropdown lists auto views', () => {
    const store = makeStore();
    render(() => <ViewSelector store={store} onOpenManage={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: /default/i }));

    const menu = screen.getByRole('menu');
    // auto:default and auto:all-geometry should be present
    expect(menu.textContent).toContain('Default');
    // The all-geometry view name from autoViewGenerator — check case-insensitively
    expect(menu.textContent?.toLowerCase()).toContain('all geometry');
  });

  it('dropdown lists user views after auto views', () => {
    const store = makeStore();
    let uid!: string;
    createRoot(() => {
      uid = store.createView('My View');
    });
    render(() => <ViewSelector store={store} onOpenManage={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: /default/i }));

    const menu = screen.getByRole('menu');
    expect(menu.textContent).toContain('My View');
  });

  it('clicking a view entry calls switchView and closes the dropdown', () => {
    const store = makeStore();
    const switchSpy = vi.spyOn(store, 'switchView');
    render(() => <ViewSelector store={store} onOpenManage={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: /default/i }));

    const allGeometryItem = screen.getByRole('menuitem', { name: /all geometry/i });
    fireEvent.click(allGeometryItem);

    expect(switchSpy).toHaveBeenCalledWith('auto:all-geometry');
    // Dropdown should close
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('pressing Escape closes the dropdown', () => {
    const store = makeStore();
    render(() => <ViewSelector store={store} onOpenManage={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: /default/i }));
    expect(screen.getByRole('menu')).toBeTruthy();

    fireEvent.keyDown(document, { key: 'Escape', bubbles: true });
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('click-outside closes the dropdown', () => {
    const store = makeStore();
    const { container } = render(() => (
      <div>
        <div data-testid="outside">outside</div>
        <ViewSelector store={store} onOpenManage={vi.fn()} />
      </div>
    ));
    fireEvent.click(screen.getByRole('button', { name: /default/i }));
    expect(screen.getByRole('menu')).toBeTruthy();

    fireEvent.mouseDown(screen.getByTestId('outside'));
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('clicking the trigger again while open closes the dropdown', () => {
    const store = makeStore();
    render(() => <ViewSelector store={store} onOpenManage={vi.fn()} />);
    const trigger = screen.getByRole('button', { name: /default/i });
    fireEvent.click(trigger);
    expect(screen.getByRole('menu')).toBeTruthy();
    fireEvent.click(trigger);
    expect(screen.queryByRole('menu')).toBeNull();
  });
});

describe('ViewSelector — Organize views footer', () => {
  it('dropdown has an "Organize views…" footer item that calls onOpenManage', () => {
    const onOpenManage = vi.fn();
    const store = makeStore();
    render(() => <ViewSelector store={store} onOpenManage={onOpenManage} />);
    fireEvent.click(screen.getByRole('button', { name: /default/i }));

    const organizeBtn = screen.getByRole('menuitem', { name: /organize views/i });
    expect(organizeBtn).toBeTruthy();
    fireEvent.click(organizeBtn);
    expect(onOpenManage).toHaveBeenCalledOnce();
  });
});

describe('ViewSelector — modified marker', () => {
  it('user view with modified: true shows a modified marker', () => {
    const store = makeStore();
    // Trigger COW by mutating while on auto view → creates "{autoName} (modified)"
    createRoot(() => {
      store.setVisibility('Root.A', 'hidden');
    });
    const cowId = store.state.activeViewId;
    expect(store.state.views[cowId].modified).toBe(true);

    // Switch back to default so the selector trigger shows the default name
    createRoot(() => { store.setActiveView('auto:default'); });

    render(() => <ViewSelector store={store} onOpenManage={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: /default/i }));

    const menu = screen.getByRole('menu');
    // The COW view should have a modified marker (any element with "modified" in data
    // attribute or class, or a dedicated marker element)
    const cowRow = screen.getByRole('menuitem', { name: /default \(modified\)/i });
    expect(cowRow).toBeTruthy();
    // The modified marker should be present
    expect(cowRow.querySelector('[data-modified]') ?? cowRow.closest('[data-modified]') ?? menu.querySelector('[data-modified]')).toBeTruthy();
  });
});
