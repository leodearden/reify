/**
 * Tests for <ViewManageModal> component (step-23).
 *
 * ViewManageModal shows a list of user views with rename / delete / duplicate /
 * reorder affordances.  It is a modal dialog that closes on Escape, overlay
 * click, and close button.
 */
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { createRoot } from 'solid-js';
import { ViewManageModal } from '../panels/ViewManageModal';
import { createViewStateStore } from '../stores/viewStateStore';
import { makeNode } from './test-utils';

function makeTree() {
  return [
    makeNode({
      entity_path: 'Root',
      kind: 'structure',
      children: [
        makeNode({ entity_path: 'Root.A', kind: 'param' }),
      ],
    }),
  ];
}

function makeStoreWithViews() {
  let store: ReturnType<typeof createViewStateStore>;
  let ids: string[] = [];
  createRoot(() => {
    store = createViewStateStore();
    store.regenerateAutoViews(makeTree());
    ids.push(store.createView('Alpha'));
    ids.push(store.createView('Beta'));
    ids.push(store.createView('Gamma'));
  });
  return { store: store!, ids };
}

describe('ViewManageModal — dialog structure', () => {
  it('renders role="dialog" with aria-modal="true" when open', () => {
    const { store } = makeStoreWithViews();
    render(() => <ViewManageModal store={store} open={true} onClose={vi.fn()} />);
    const dialog = screen.getByRole('dialog');
    expect(dialog).toBeTruthy();
    expect(dialog.getAttribute('aria-modal')).toBe('true');
  });

  it('does NOT render when open=false', () => {
    const { store } = makeStoreWithViews();
    render(() => <ViewManageModal store={store} open={false} onClose={vi.fn()} />);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('first focusable element receives autofocus', async () => {
    const { store } = makeStoreWithViews();
    render(() => <ViewManageModal store={store} open={true} onClose={vi.fn()} />);
    // After queueMicrotask, focus should be inside the dialog
    await new Promise((r) => queueMicrotask(r as () => void));
    const dialog = screen.getByRole('dialog');
    const focusable = dialog.querySelector('button, input, [tabindex]:not([tabindex="-1"])');
    expect(focusable).toBeTruthy();
    expect(document.activeElement).toBe(focusable);
  });
});

describe('ViewManageModal — user view list', () => {
  it('lists all user views by name (each as a rename input)', () => {
    const { store } = makeStoreWithViews();
    render(() => <ViewManageModal store={store} open={true} onClose={vi.fn()} />);
    // User views are rendered as rename inputs whose value is the view name
    expect(screen.getByDisplayValue('Alpha')).toBeTruthy();
    expect(screen.getByDisplayValue('Beta')).toBeTruthy();
    expect(screen.getByDisplayValue('Gamma')).toBeTruthy();
  });

  it('does NOT list auto views', () => {
    const { store } = makeStoreWithViews();
    render(() => <ViewManageModal store={store} open={true} onClose={vi.fn()} />);
    // "Default" and "All geometry" are auto views — must not appear as list rows
    // (they may appear in the title/heading, so scope to the list)
    const list = screen.getByRole('list');
    expect(list.textContent).not.toContain('auto:default');
    // auto view id must not be visible; names like "Default" might appear via
    // other UI but the id prefix must not
    expect(list.innerHTML).not.toContain('auto:');
  });
});

describe('ViewManageModal — rename', () => {
  it('inline rename input shows the current name and accepts Enter to commit', () => {
    const { store, ids } = makeStoreWithViews();
    const renameSpy = vi.spyOn(store, 'renameView');
    render(() => <ViewManageModal store={store} open={true} onClose={vi.fn()} />);

    // Find rename input for Alpha
    const input = screen.getByDisplayValue('Alpha') as HTMLInputElement;
    expect(input).toBeTruthy();

    // Change and commit
    fireEvent.input(input, { target: { value: 'Alpha Renamed' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(renameSpy).toHaveBeenCalledWith(ids[0], 'Alpha Renamed');
  });

  it('pressing Escape on rename input reverts to original name without calling renameView', () => {
    const { store, ids } = makeStoreWithViews();
    const renameSpy = vi.spyOn(store, 'renameView');
    render(() => <ViewManageModal store={store} open={true} onClose={vi.fn()} />);

    const input = screen.getByDisplayValue('Alpha') as HTMLInputElement;
    fireEvent.input(input, { target: { value: 'Changed' } });
    fireEvent.keyDown(input, { key: 'Escape' });
    // renameView must NOT have been called
    expect(renameSpy).not.toHaveBeenCalled();
    // Input reverts to original value
    expect((screen.getByDisplayValue('Alpha') as HTMLInputElement).value).toBe('Alpha');
  });
});

describe('ViewManageModal — delete', () => {
  it('clicking the delete button for a view calls deleteView', () => {
    const { store, ids } = makeStoreWithViews();
    const deleteSpy = vi.spyOn(store, 'deleteView');
    render(() => <ViewManageModal store={store} open={true} onClose={vi.fn()} />);

    // Find delete button for Alpha row
    const alphaRow = screen.getByDisplayValue('Alpha').closest('li') ??
      screen.getByDisplayValue('Alpha').closest('[data-view-id]');
    expect(alphaRow).toBeTruthy();
    const deleteBtn = alphaRow!.querySelector('[data-action="delete"]') as HTMLButtonElement;
    expect(deleteBtn).toBeTruthy();
    fireEvent.click(deleteBtn);
    expect(deleteSpy).toHaveBeenCalledWith(ids[0]);
  });
});

describe('ViewManageModal — duplicate', () => {
  it('clicking the duplicate button for a view calls duplicateView', () => {
    const { store, ids } = makeStoreWithViews();
    const dupSpy = vi.spyOn(store, 'duplicateView');
    render(() => <ViewManageModal store={store} open={true} onClose={vi.fn()} />);

    const betaRow = screen.getByDisplayValue('Beta').closest('li') ??
      screen.getByDisplayValue('Beta').closest('[data-view-id]');
    const dupBtn = betaRow!.querySelector('[data-action="duplicate"]') as HTMLButtonElement;
    expect(dupBtn).toBeTruthy();
    fireEvent.click(dupBtn);
    expect(dupSpy).toHaveBeenCalledWith(ids[1]);
  });
});

describe('ViewManageModal — reorder', () => {
  it('up arrow on non-first item calls reorderUserViews with item moved up', () => {
    const { store, ids } = makeStoreWithViews();
    const reorderSpy = vi.spyOn(store, 'reorderUserViews');
    render(() => <ViewManageModal store={store} open={true} onClose={vi.fn()} />);

    // Find move-up button for Beta (index 1)
    const betaRow = screen.getByDisplayValue('Beta').closest('li') ??
      screen.getByDisplayValue('Beta').closest('[data-view-id]');
    const upBtn = betaRow!.querySelector('[data-action="move-up"]') as HTMLButtonElement;
    expect(upBtn).toBeTruthy();
    fireEvent.click(upBtn);
    // Beta (ids[1]) should now be before Alpha (ids[0])
    expect(reorderSpy).toHaveBeenCalledWith([ids[1], ids[0], ids[2]]);
  });

  it('down arrow on non-last item calls reorderUserViews with item moved down', () => {
    const { store, ids } = makeStoreWithViews();
    const reorderSpy = vi.spyOn(store, 'reorderUserViews');
    render(() => <ViewManageModal store={store} open={true} onClose={vi.fn()} />);

    // Find move-down button for Beta (index 1)
    const betaRow = screen.getByDisplayValue('Beta').closest('li') ??
      screen.getByDisplayValue('Beta').closest('[data-view-id]');
    const downBtn = betaRow!.querySelector('[data-action="move-down"]') as HTMLButtonElement;
    expect(downBtn).toBeTruthy();
    fireEvent.click(downBtn);
    expect(reorderSpy).toHaveBeenCalledWith([ids[0], ids[2], ids[1]]);
  });
});

describe('ViewManageModal — close affordances', () => {
  it('pressing Escape calls onClose', () => {
    const onClose = vi.fn();
    const { store } = makeStoreWithViews();
    render(() => <ViewManageModal store={store} open={true} onClose={onClose} />);

    fireEvent.keyDown(screen.getByRole('dialog').parentElement!, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('clicking the overlay (outside dialog) calls onClose', () => {
    const onClose = vi.fn();
    const { store } = makeStoreWithViews();
    const { container } = render(() =>
      <ViewManageModal store={store} open={true} onClose={onClose} />,
    );
    // Click directly on the overlay (not on the dialog content)
    const overlay = container.querySelector('[data-testid="view-manage-overlay"]');
    expect(overlay).toBeTruthy();
    fireEvent.click(overlay!);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('clicking the close button calls onClose', () => {
    const onClose = vi.fn();
    const { store } = makeStoreWithViews();
    render(() => <ViewManageModal store={store} open={true} onClose={onClose} />);
    const closeBtn = screen.getByRole('button', { name: /close/i });
    fireEvent.click(closeBtn);
    expect(onClose).toHaveBeenCalledOnce();
  });
});
