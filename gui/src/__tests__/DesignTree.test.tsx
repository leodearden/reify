import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, within } from '@solidjs/testing-library';
import { createRoot } from 'solid-js';
import { DesignTree } from '../panels/DesignTree';
import { createViewStateStore } from '../stores/viewStateStore';
import type { EntityTreeNode } from '../types';
import { makeNode } from './test-utils';

function makeStore(nodes: EntityTreeNode[]) {
  let store: ReturnType<typeof createViewStateStore>;
  createRoot(() => {
    store = createViewStateStore();
    store.setTree(nodes);
  });
  return store!;
}

describe('DesignTree — baseline rendering', () => {
  it('renders with data-testid="design-tree"', () => {
    const store = makeStore([]);
    render(() => <DesignTree tree={[]} viewStateStore={store} />);
    expect(screen.getByTestId('design-tree')).toBeTruthy();
  });

  it('renders each top-level node as a row', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
    ];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    // Scope to the specific row so a regression to full entity_path doesn't silently pass.
    expect(within(screen.getByTestId('tree-row-Root.A')).getByText('A')).toBeTruthy();
    expect(within(screen.getByTestId('tree-row-Root.B')).getByText('B')).toBeTruthy();
  });

  it('rows show the last path segment as name', () => {
    const nodes = [makeNode({ entity_path: 'MyDesign.Bracket' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    expect(screen.getByText('Bracket')).toBeTruthy();
  });

  it('each row has an eye icon button', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    const tree = screen.getByTestId('design-tree');
    const eyeBtn = tree.querySelector('[data-testid="eye-icon-Root.A"]');
    expect(eyeBtn).toBeTruthy();
  });

  it('child rows are not rendered until parent is expanded', () => {
    const nodes = [
      makeNode({
        entity_path: 'Root.A',
        children: [makeNode({ entity_path: 'Root.A.a1' })],
      }),
    ];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    // a1 should not appear initially
    expect(screen.queryByText('a1')).toBeNull();
  });

  it('clicking the chevron expands a node to show children', () => {
    const nodes = [
      makeNode({
        entity_path: 'Root.A',
        children: [makeNode({ entity_path: 'Root.A.a1' })],
      }),
    ];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    const chevron = screen.getByTestId('chevron-Root.A');
    fireEvent.click(chevron);
    expect(screen.getByText('a1')).toBeTruthy();
  });

  it('eye icon has aria-label reflecting effective visibility', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    // Default effective is 'show'
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    const eyeBtn = screen.getByTestId('eye-icon-Root.A');
    expect(eyeBtn.getAttribute('aria-label')).toBe('show');
  });

  it('eye icon aria-label updates reactively after setVisibility', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    // Initial render: default effective visibility is 'show'
    const eyeBtn = screen.getByTestId('eye-icon-Root.A');
    expect(eyeBtn.getAttribute('aria-label')).toBe('show');
    // After mutation, aria-label should reactively update
    store.setVisibility('Root.A', 'ghost', false);
    expect(screen.getByTestId('eye-icon-Root.A').getAttribute('aria-label')).toBe('ghost');
  });
});

describe('DesignTree — eye icon cycle', () => {
  it('clicking eye icon calls cycleCascading → show becomes ghost', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    const eyeBtn = screen.getByTestId('eye-icon-Root.A');
    fireEvent.click(eyeBtn);
    expect(store.state.explicit['Root.A']).toBe('ghost');
  });

  it('clicking eye icon twice → hidden', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    const eyeBtn = screen.getByTestId('eye-icon-Root.A');
    fireEvent.click(eyeBtn);
    fireEvent.click(eyeBtn);
    expect(store.state.explicit['Root.A']).toBe('hidden');
  });

  it('eye icon aria-label and glyph update reactively on click', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    const eyeBtn = screen.getByTestId('eye-icon-Root.A');
    // Initial: show
    expect(eyeBtn.getAttribute('aria-label')).toBe('show');
    // Click once: show → ghost
    fireEvent.click(eyeBtn);
    expect(eyeBtn.getAttribute('aria-label')).toBe('ghost');
    expect(eyeBtn.textContent).toContain('◑');
    // Click again: ghost → hidden
    fireEvent.click(eyeBtn);
    expect(eyeBtn.getAttribute('aria-label')).toBe('hidden');
    expect(eyeBtn.textContent).toContain('○');
  });

  it('cycle cascades: descendant explicit becomes null', () => {
    const nodes = [
      makeNode({
        entity_path: 'Root.A',
        children: [makeNode({ entity_path: 'Root.A.a1' })],
      }),
    ];
    const store = makeStore(nodes);
    store.setVisibility('Root.A.a1', 'hidden', false);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    const eyeBtn = screen.getByTestId('eye-icon-Root.A');
    fireEvent.click(eyeBtn); // show → ghost, cascade
    expect(store.state.explicit['Root.A.a1']).toBeUndefined();
    expect(store.getEffectiveVisibility('Root.A.a1')).toBe('ghost');
  });
});

describe('DesignTree — context menu', () => {
  it('right-clicking a row opens the context menu', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    const row = screen.getByTestId('tree-row-Root.A');
    fireEvent.contextMenu(row);
    expect(screen.getByTestId('design-tree-context-menu')).toBeTruthy();
  });

  it('"Hide this and children" calls setVisibility with hidden+cascade', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    fireEvent.contextMenu(screen.getByTestId('tree-row-Root.A'));
    fireEvent.click(screen.getByTestId('ctx-hide-cascade'));
    expect(store.state.explicit['Root.A']).toBe('hidden');
  });

  it('"Reset to default" calls resetToInherit', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    store.setVisibility('Root.A', 'hidden', false);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    fireEvent.contextMenu(screen.getByTestId('tree-row-Root.A'));
    fireEvent.click(screen.getByTestId('ctx-reset'));
    expect(store.state.explicit['Root.A']).toBeUndefined();
  });

  it('"Show only this" calls showOnly(path, true)', () => {
    const nodes = [
      makeNode({ entity_path: 'A' }),
      makeNode({ entity_path: 'B' }),
    ];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    fireEvent.contextMenu(screen.getByTestId('tree-row-A'));
    fireEvent.click(screen.getByTestId('ctx-show-only'));
    expect(store.state.explicit['A']).toBe('show');
    expect(store.state.explicit['B']).toBe('hidden');
  });

  it('menu dismisses on subsequent document click', async () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    fireEvent.contextMenu(screen.getByTestId('tree-row-Root.A'));
    expect(screen.getByTestId('design-tree-context-menu')).toBeTruthy();
    fireEvent.click(document.body);
    expect(screen.queryByTestId('design-tree-context-menu')).toBeNull();
  });

  it('pressing Escape while menu is open dismisses it', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    fireEvent.contextMenu(screen.getByTestId('tree-row-Root.A'));
    expect(screen.getByTestId('design-tree-context-menu')).toBeTruthy();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByTestId('design-tree-context-menu')).toBeNull();
  });

  it('clicking eye-icon on another row while menu is open both cycles visibility and dismisses menu', () => {
    // Pins the capture-phase dismiss contract: dismiss must happen even when the
    // target's own handler calls e.stopPropagation() (eye-icon does), and the
    // target's action must still fire.
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
    ];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    fireEvent.contextMenu(screen.getByTestId('tree-row-Root.A'));
    expect(screen.getByTestId('design-tree-context-menu')).toBeTruthy();
    const before = store.state.explicit['Root.B'];
    fireEvent.click(screen.getByTestId('eye-icon-Root.B'));
    expect(store.state.explicit['Root.B']).not.toBe(before);
    expect(screen.queryByTestId('design-tree-context-menu')).toBeNull();
  });

  it('contextmenu on successive rows does not accumulate document click listeners', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
    ];
    const store = makeStore(nodes);
    const addSpy = vi.spyOn(document, 'addEventListener');
    try {
      render(() => <DesignTree tree={nodes} viewStateStore={store} />);
      // Capture baseline after render — onMount has already registered its listener(s).
      // Asserting count is unchanged (not merely ≤ 1) proves no accumulation while
      // also requiring that at least one listener exists (0 would mean dismiss is broken).
      const baselineClickAdds = addSpy.mock.calls.filter((c) => c[0] === 'click').length;
      fireEvent.contextMenu(screen.getByTestId('tree-row-Root.A'));
      fireEvent.contextMenu(screen.getByTestId('tree-row-Root.B'));
      expect(addSpy.mock.calls.filter((c) => c[0] === 'click').length).toBe(baselineClickAdds);
      fireEvent.click(document.body);
      expect(screen.queryByTestId('design-tree-context-menu')).toBeNull();
    } finally {
      addSpy.mockRestore();
    }
  });

  it('unmount removes document click and keydown listeners (no leak)', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    const addSpy = vi.spyOn(document, 'addEventListener');
    const removeSpy = vi.spyOn(document, 'removeEventListener');
    try {
      const addsBefore = {
        click: addSpy.mock.calls.filter((c) => c[0] === 'click').length,
        keydown: addSpy.mock.calls.filter((c) => c[0] === 'keydown').length,
      };
      const removesBefore = {
        click: removeSpy.mock.calls.filter((c) => c[0] === 'click').length,
        keydown: removeSpy.mock.calls.filter((c) => c[0] === 'keydown').length,
      };
      const { unmount } = render(() => <DesignTree tree={nodes} viewStateStore={store} />);
      const addsAfterRender = {
        click: addSpy.mock.calls.filter((c) => c[0] === 'click').length,
        keydown: addSpy.mock.calls.filter((c) => c[0] === 'keydown').length,
      };
      unmount();
      const removesAfterUnmount = {
        click: removeSpy.mock.calls.filter((c) => c[0] === 'click').length,
        keydown: removeSpy.mock.calls.filter((c) => c[0] === 'keydown').length,
      };
      // Net added by DesignTree across mount→unmount must equal net removed for each
      // event type. This scopes the assertion to DesignTree's own delta and is immune
      // to unrelated listeners from solid-js / testing harness / jsdom.
      expect(addsAfterRender.click - addsBefore.click).toBe(removesAfterUnmount.click - removesBefore.click);
      expect(addsAfterRender.keydown - addsBefore.keydown).toBe(removesAfterUnmount.keydown - removesBefore.keydown);
      // And DesignTree must have registered at least one listener of each type —
      // otherwise dismiss/Escape are silently broken.
      expect(addsAfterRender.click - addsBefore.click).toBeGreaterThan(0);
      expect(addsAfterRender.keydown - addsBefore.keydown).toBeGreaterThan(0);
    } finally {
      addSpy.mockRestore();
      removeSpy.mockRestore();
    }
  });
});

describe('DesignTree — multi-selection highlight', () => {
  it('rows in selectedEntities get data-selected="true"', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
      makeNode({ entity_path: 'Root.C' }),
    ];
    const store = makeStore(nodes);
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        selectedEntities={['Root.A', 'Root.B']}
      />
    ));
    expect(screen.getByTestId('tree-row-Root.A').getAttribute('data-selected')).toBe('true');
    expect(screen.getByTestId('tree-row-Root.B').getAttribute('data-selected')).toBe('true');
  });

  it('rows NOT in selectedEntities do not have data-selected="true"', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
      makeNode({ entity_path: 'Root.C' }),
    ];
    const store = makeStore(nodes);
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        selectedEntities={['Root.A', 'Root.B']}
      />
    ));
    expect(screen.getByTestId('tree-row-Root.C').getAttribute('data-selected')).not.toBe('true');
  });

  it('backward-compat: selectedEntity (legacy) marks that single row selected', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
    ];
    const store = makeStore(nodes);
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        selectedEntity="Root.A"
      />
    ));
    expect(screen.getByTestId('tree-row-Root.A').getAttribute('data-selected')).toBe('true');
    expect(screen.getByTestId('tree-row-Root.B').getAttribute('data-selected')).not.toBe('true');
  });

  it('selectedEntities takes precedence over selectedEntity when both provided', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
    ];
    const store = makeStore(nodes);
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        selectedEntity="Root.A"
        selectedEntities={['Root.A', 'Root.B']}
      />
    ));
    // Both should be selected because selectedEntities overrides
    expect(screen.getByTestId('tree-row-Root.A').getAttribute('data-selected')).toBe('true');
    expect(screen.getByTestId('tree-row-Root.B').getAttribute('data-selected')).toBe('true');
  });
});

describe('DesignTree — modifier click routing', () => {
  it('plain click calls onSelect with (path, { ctrl: false, shift: false })', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    const onSelect = vi.fn();
    render(() => (
      <DesignTree tree={nodes} viewStateStore={store} onSelect={onSelect} />
    ));
    fireEvent.click(screen.getByTestId('tree-row-Root.A'));
    expect(onSelect).toHaveBeenCalledOnce();
    expect(onSelect).toHaveBeenCalledWith('Root.A', { ctrl: false, shift: false });
  });

  it('Ctrl+click calls onSelect with (path, { ctrl: true, shift: false })', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    const onSelect = vi.fn();
    render(() => (
      <DesignTree tree={nodes} viewStateStore={store} onSelect={onSelect} />
    ));
    fireEvent.click(screen.getByTestId('tree-row-Root.A'), { ctrlKey: true });
    expect(onSelect).toHaveBeenCalledOnce();
    expect(onSelect).toHaveBeenCalledWith('Root.A', { ctrl: true, shift: false });
  });

  it('Meta+click (Mac) treated as ctrl, passes { ctrl: true, shift: false }', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    const onSelect = vi.fn();
    render(() => (
      <DesignTree tree={nodes} viewStateStore={store} onSelect={onSelect} />
    ));
    fireEvent.click(screen.getByTestId('tree-row-Root.A'), { metaKey: true });
    expect(onSelect).toHaveBeenCalledOnce();
    expect(onSelect).toHaveBeenCalledWith('Root.A', { ctrl: true, shift: false });
  });

  it('Shift+click calls onSelect with (path, { ctrl: false, shift: true })', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    const onSelect = vi.fn();
    render(() => (
      <DesignTree tree={nodes} viewStateStore={store} onSelect={onSelect} />
    ));
    fireEvent.click(screen.getByTestId('tree-row-Root.A'), { shiftKey: true });
    expect(onSelect).toHaveBeenCalledOnce();
    expect(onSelect).toHaveBeenCalledWith('Root.A', { ctrl: false, shift: true });
  });

  it('single-arg onSelect handler (ignores second arg) is still invoked', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    const captured: string[] = [];
    // Callback that accepts only one arg — TypeScript callers can ignore extra args
    const onSelect = (path: string) => { captured.push(path); };
    render(() => (
      <DesignTree tree={nodes} viewStateStore={store} onSelect={onSelect as any} />
    ));
    fireEvent.click(screen.getByTestId('tree-row-Root.A'));
    expect(captured).toEqual(['Root.A']);
  });
});

describe('DesignTree — range select', () => {
  // Tree: Root.A, Root.B, Root.C, Root.D as siblings (all visible, none expanded yet)
  function makeFlatTree() {
    return [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
      makeNode({ entity_path: 'Root.C' }),
      makeNode({ entity_path: 'Root.D' }),
    ];
  }

  it('Shift+click with anchorEntity calls onRangeSelect with the slice (A→C)', () => {
    const nodes = makeFlatTree();
    const store = makeStore(nodes);
    const onSelect = vi.fn();
    const onRangeSelect = vi.fn();
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        anchorEntity="Root.A"
        onSelect={onSelect}
        onRangeSelect={onRangeSelect}
      />
    ));
    fireEvent.click(screen.getByTestId('tree-row-Root.C'), { shiftKey: true });
    expect(onRangeSelect).toHaveBeenCalledOnce();
    expect(onRangeSelect).toHaveBeenCalledWith(['Root.A', 'Root.B', 'Root.C']);
    // onSelect must NOT be called when onRangeSelect is used
    expect(onSelect).not.toHaveBeenCalled();
  });

  it('Shift+click with no anchorEntity falls back to onSelect(path, { shift: true })', () => {
    const nodes = makeFlatTree();
    const store = makeStore(nodes);
    const onSelect = vi.fn();
    const onRangeSelect = vi.fn();
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        onSelect={onSelect}
        onRangeSelect={onRangeSelect}
      />
    ));
    fireEvent.click(screen.getByTestId('tree-row-Root.C'), { shiftKey: true });
    expect(onSelect).toHaveBeenCalledOnce();
    expect(onSelect).toHaveBeenCalledWith('Root.C', { ctrl: false, shift: true });
    expect(onRangeSelect).not.toHaveBeenCalled();
  });

  it('range respects expansion: collapsed children are NOT included', () => {
    // Root.B has children b1,b2 but Root.B is NOT expanded → b1,b2 excluded
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({
        entity_path: 'Root.B',
        children: [
          makeNode({ entity_path: 'Root.B.b1' }),
          makeNode({ entity_path: 'Root.B.b2' }),
        ],
      }),
      makeNode({ entity_path: 'Root.C' }),
    ];
    const store = makeStore(nodes);
    const onRangeSelect = vi.fn();
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        anchorEntity="Root.A"
        onRangeSelect={onRangeSelect}
      />
    ));
    // Root.B is not expanded → Range A→C is [Root.A, Root.B, Root.C]
    fireEvent.click(screen.getByTestId('tree-row-Root.C'), { shiftKey: true });
    expect(onRangeSelect).toHaveBeenCalledWith(['Root.A', 'Root.B', 'Root.C']);
  });

  it('range order is ascending (visible flat order) regardless of click direction (C→A)', () => {
    const nodes = makeFlatTree();
    const store = makeStore(nodes);
    const onRangeSelect = vi.fn();
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        anchorEntity="Root.C"
        onRangeSelect={onRangeSelect}
      />
    ));
    // Clicking A with anchor=C should still yield ascending order [A, B, C]
    fireEvent.click(screen.getByTestId('tree-row-Root.A'), { shiftKey: true });
    expect(onRangeSelect).toHaveBeenCalledWith(['Root.A', 'Root.B', 'Root.C']);
  });
});

describe('DesignTree — select all', () => {
  it('Ctrl+A calls onSelectAll with all visible flat paths', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
    ];
    const store = makeStore(nodes);
    const onSelectAll = vi.fn();
    render(() => (
      <DesignTree tree={nodes} viewStateStore={store} onSelectAll={onSelectAll} />
    ));
    const treeRoot = screen.getByTestId('design-tree');
    fireEvent.keyDown(treeRoot, { key: 'a', ctrlKey: true });
    expect(onSelectAll).toHaveBeenCalledOnce();
    expect(onSelectAll).toHaveBeenCalledWith(['Root.A', 'Root.B']);
  });

  it('Ctrl+A excludes collapsed children (only visible paths)', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({
        entity_path: 'Root.B',
        children: [makeNode({ entity_path: 'Root.B.b1' })],
      }),
    ];
    const store = makeStore(nodes);
    const onSelectAll = vi.fn();
    render(() => (
      <DesignTree tree={nodes} viewStateStore={store} onSelectAll={onSelectAll} />
    ));
    const treeRoot = screen.getByTestId('design-tree');
    // Root.B is not expanded → b1 is not visible
    fireEvent.keyDown(treeRoot, { key: 'a', ctrlKey: true });
    expect(onSelectAll).toHaveBeenCalledWith(['Root.A', 'Root.B']);
  });

  it('Ctrl+A without onSelectAll prop is a no-op (no throw)', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    const treeRoot = screen.getByTestId('design-tree');
    expect(() => fireEvent.keyDown(treeRoot, { key: 'a', ctrlKey: true })).not.toThrow();
  });

  it('Meta+A (Mac) also triggers onSelectAll', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    const onSelectAll = vi.fn();
    render(() => (
      <DesignTree tree={nodes} viewStateStore={store} onSelectAll={onSelectAll} />
    ));
    const treeRoot = screen.getByTestId('design-tree');
    fireEvent.keyDown(treeRoot, { key: 'a', metaKey: true });
    expect(onSelectAll).toHaveBeenCalledOnce();
    expect(onSelectAll).toHaveBeenCalledWith(['Root.A']);
  });

  it('Ctrl+A calls e.preventDefault() (suppresses browser Select-All)', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    const onSelectAll = vi.fn();
    render(() => (
      <DesignTree tree={nodes} viewStateStore={store} onSelectAll={onSelectAll} />
    ));
    const treeRoot = screen.getByTestId('design-tree');
    const event = new KeyboardEvent('keydown', { key: 'a', ctrlKey: true, bubbles: true, cancelable: true });
    const preventDefaultSpy = vi.spyOn(event, 'preventDefault');
    treeRoot.dispatchEvent(event);
    expect(preventDefaultSpy).toHaveBeenCalled();
  });
});

describe('DesignTree — bulk eye-icon', () => {
  it('clicking eye-icon on a row in a multi-selection cycles ALL selected rows', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
      makeNode({ entity_path: 'Root.C' }),
    ];
    const store = makeStore(nodes);
    // Root.A and Root.B are selected
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        selectedEntities={['Root.A', 'Root.B']}
      />
    ));
    // Click eye-icon on Root.A (which is in the selected set)
    fireEvent.click(screen.getByTestId('eye-icon-Root.A'));
    // Both Root.A and Root.B should be cycled (show → ghost)
    expect(store.state.explicit['Root.A']).toBe('ghost');
    expect(store.state.explicit['Root.B']).toBe('ghost');
    // Root.C (not selected) should be unchanged
    expect(store.state.explicit['Root.C']).toBeUndefined();
  });

  it('clicking eye-icon on a row NOT in the selection cycles only that row', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
      makeNode({ entity_path: 'Root.C' }),
    ];
    const store = makeStore(nodes);
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        selectedEntities={['Root.A', 'Root.B']}
      />
    ));
    // Click eye-icon on Root.C (NOT in selection)
    fireEvent.click(screen.getByTestId('eye-icon-Root.C'));
    // Only Root.C should be cycled
    expect(store.state.explicit['Root.C']).toBe('ghost');
    // Root.A and Root.B unchanged
    expect(store.state.explicit['Root.A']).toBeUndefined();
    expect(store.state.explicit['Root.B']).toBeUndefined();
  });

  it('clicking eye-icon with single-item selectedEntities cycles only that one row', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
    ];
    const store = makeStore(nodes);
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        selectedEntities={['Root.A']}
      />
    ));
    fireEvent.click(screen.getByTestId('eye-icon-Root.A'));
    expect(store.state.explicit['Root.A']).toBe('ghost');
    expect(store.state.explicit['Root.B']).toBeUndefined();
  });

  it('backward-compat: no selectedEntities prop → clicking eye cycles just that row', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
    ];
    const store = makeStore(nodes);
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        selectedEntity="Root.A"
      />
    ));
    fireEvent.click(screen.getByTestId('eye-icon-Root.A'));
    expect(store.state.explicit['Root.A']).toBe('ghost');
    expect(store.state.explicit['Root.B']).toBeUndefined();
  });
});

describe('DesignTree — keyboard shortcuts', () => {
  it('pressing H with selected entity sets hidden+cascade', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} selectedEntity="Root.A" />);
    const treeRoot = screen.getByTestId('design-tree');
    fireEvent.keyDown(treeRoot, { key: 'h' });
    expect(store.state.explicit['Root.A']).toBe('hidden');
  });

  it('pressing G with selected entity sets ghost+cascade', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} selectedEntity="Root.A" />);
    const treeRoot = screen.getByTestId('design-tree');
    fireEvent.keyDown(treeRoot, { key: 'g' });
    expect(store.state.explicit['Root.A']).toBe('ghost');
  });

  it('pressing S with selected entity sets show+cascade', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    store.setVisibility('Root.A', 'hidden', false);
    render(() => <DesignTree tree={nodes} viewStateStore={store} selectedEntity="Root.A" />);
    const treeRoot = screen.getByTestId('design-tree');
    fireEvent.keyDown(treeRoot, { key: 's' });
    expect(store.state.explicit['Root.A']).toBe('show');
  });

  it('pressing Enter with selected entity sets show+cascade', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    store.setVisibility('Root.A', 'hidden', false);
    render(() => <DesignTree tree={nodes} viewStateStore={store} selectedEntity="Root.A" />);
    const treeRoot = screen.getByTestId('design-tree');
    fireEvent.keyDown(treeRoot, { key: 'Enter' });
    expect(store.state.explicit['Root.A']).toBe('show');
  });

  it('pressing H with no selected entity is a no-op', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    const treeRoot = screen.getByTestId('design-tree');
    fireEvent.keyDown(treeRoot, { key: 'h' });
    expect(store.state.explicit['Root.A']).toBeUndefined();
  });

  it('uppercase H (caps-lock) also sets hidden', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} selectedEntity="Root.A" />);
    const treeRoot = screen.getByTestId('design-tree');
    fireEvent.keyDown(treeRoot, { key: 'H' });
    expect(store.state.explicit['Root.A']).toBe('hidden');
  });

  it('Ctrl+H is a no-op (does not override browser shortcut)', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} selectedEntity="Root.A" />);
    const treeRoot = screen.getByTestId('design-tree');
    fireEvent.keyDown(treeRoot, { key: 'h', ctrlKey: true });
    expect(store.state.explicit['Root.A']).toBeUndefined();
  });

  it('Meta+H is a no-op', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    render(() => <DesignTree tree={nodes} viewStateStore={store} selectedEntity="Root.A" />);
    const treeRoot = screen.getByTestId('design-tree');
    fireEvent.keyDown(treeRoot, { key: 'h', metaKey: true });
    expect(store.state.explicit['Root.A']).toBeUndefined();
  });

  it('pressing H with multiple entities selected hides ALL of them', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
      makeNode({ entity_path: 'Root.C' }),
    ];
    const store = makeStore(nodes);
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        selectedEntities={['Root.A', 'Root.B']}
      />
    ));
    const treeRoot = screen.getByTestId('design-tree');
    fireEvent.keyDown(treeRoot, { key: 'h' });
    expect(store.state.explicit['Root.A']).toBe('hidden');
    expect(store.state.explicit['Root.B']).toBe('hidden');
    // Root.C (not selected) must remain unchanged
    expect(store.state.explicit['Root.C']).toBeUndefined();
  });

  it('pressing G with multiple entities selected ghosts ALL of them', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
    ];
    const store = makeStore(nodes);
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        selectedEntities={['Root.A', 'Root.B']}
      />
    ));
    const treeRoot = screen.getByTestId('design-tree');
    fireEvent.keyDown(treeRoot, { key: 'g' });
    expect(store.state.explicit['Root.A']).toBe('ghost');
    expect(store.state.explicit['Root.B']).toBe('ghost');
  });

  it('pressing S with multiple entities selected shows ALL of them', () => {
    const nodes = [
      makeNode({ entity_path: 'Root.A' }),
      makeNode({ entity_path: 'Root.B' }),
    ];
    const store = makeStore(nodes);
    store.setVisibility('Root.A', 'hidden', false);
    store.setVisibility('Root.B', 'hidden', false);
    render(() => (
      <DesignTree
        tree={nodes}
        viewStateStore={store}
        selectedEntities={['Root.A', 'Root.B']}
      />
    ));
    const treeRoot = screen.getByTestId('design-tree');
    fireEvent.keyDown(treeRoot, { key: 's' });
    expect(store.state.explicit['Root.A']).toBe('show');
    expect(store.state.explicit['Root.B']).toBe('show');
  });
});
