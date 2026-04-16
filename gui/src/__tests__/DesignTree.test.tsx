import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, within } from '@solidjs/testing-library';
import { createRoot } from 'solid-js';
import { DesignTree } from '../panels/DesignTree';
import { createViewStateStore } from '../stores/viewStateStore';
import type { EntityTreeNode } from '../types';

function makeNode(overrides: Partial<EntityTreeNode> & { entity_path: string }): EntityTreeNode {
  return {
    kind: 'structure',
    type_name: null,
    has_mesh: false,
    trait_geometry: false,
    children: [],
    ...overrides,
  };
}

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

  it('eye icon aria-label updates when explicit visibility changes', () => {
    const nodes = [makeNode({ entity_path: 'Root.A' })];
    const store = makeStore(nodes);
    store.setVisibility('Root.A', 'ghost', false);
    render(() => <DesignTree tree={nodes} viewStateStore={store} />);
    const eyeBtn = screen.getByTestId('eye-icon-Root.A');
    expect(eyeBtn.getAttribute('aria-label')).toBe('ghost');
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
    expect(store.state.explicit['Root.A.a1']).toBeNull();
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
    expect(store.state.explicit['Root.A']).toBeNull();
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
});
