import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@solidjs/testing-library';
import type { GuiState } from '../types';

// Mock Tauri APIs before any component imports
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] }),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

// Mock Viewport (requires Three.js / WebGL which jsdom doesn't support)
vi.mock('../viewport', () => ({
  Viewport: (_props: any) => {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'viewport-container');
    el.textContent = 'Viewport Mock';
    return el;
  },
}));

// Mock Editor (requires CodeMirror DOM APIs)
vi.mock('../editor/Editor', () => ({
  Editor: (_props: any) => {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'editor-container');
    el.textContent = 'Editor Mock';
    return el;
  },
}));

// Mock FileTabs
vi.mock('../editor/FileTabs', () => ({
  FileTabs: (_props: any) => {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'file-tabs');
    el.textContent = 'FileTabs Mock';
    return el;
  },
}));

// Mock bridge functions
const emptyState: GuiState = { meshes: [], values: [], constraints: [], files: [] };
vi.mock('../bridge', () => ({
  getInitialState: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] }),
  setParameter: vi.fn().mockResolvedValue(undefined),
  onMeshUpdate: vi.fn().mockResolvedValue(() => {}),
  onValueUpdate: vi.fn().mockResolvedValue(() => {}),
  onConstraintUpdate: vi.fn().mockResolvedValue(() => {}),
  onEvaluationStatus: vi.fn().mockResolvedValue(() => {}),
  onMeshRemoved: vi.fn().mockResolvedValue(() => {}),
  onValueRemoved: vi.fn().mockResolvedValue(() => {}),
  onConstraintRemoved: vi.fn().mockResolvedValue(() => {}),
}));

import App from '../App';
import * as bridge from '../bridge';

beforeEach(() => {
  vi.clearAllMocks();
  // Reset getInitialState to default empty state
  vi.mocked(bridge.getInitialState).mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] });
});

afterEach(() => {
  cleanup();
});

describe('App layout wiring', () => {
  it('renders app-layout container', () => {
    render(() => <App />);
    expect(screen.getByTestId('app-layout')).toBeTruthy();
  });

  it('renders Toolbar at top', () => {
    render(() => <App />);
    expect(screen.getByTestId('toolbar')).toBeTruthy();
  });

  it('renders StatusBar at bottom', () => {
    render(() => <App />);
    expect(screen.getByTestId('status-bar')).toBeTruthy();
  });

  it('renders Viewport', () => {
    render(() => <App />);
    expect(screen.getByTestId('viewport-container')).toBeTruthy();
  });

  it('renders Editor', () => {
    render(() => <App />);
    expect(screen.getByTestId('editor-container')).toBeTruthy();
  });

  it('renders PropertyEditor', () => {
    render(() => <App />);
    expect(screen.getByTestId('property-editor')).toBeTruthy();
  });

  it('renders ConstraintPanel', () => {
    render(() => <App />);
    expect(screen.getByTestId('constraint-panel')).toBeTruthy();
  });

  it('renders Toolbar before StatusBar in DOM order', () => {
    render(() => <App />);
    const toolbar = screen.getByTestId('toolbar');
    const statusBar = screen.getByTestId('status-bar');
    // Toolbar should come before StatusBar in document order
    const comparison = toolbar.compareDocumentPosition(statusBar);
    expect(comparison & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});

describe('App resizable splitters', () => {
  it('has a vertical splitter between editor and viewport columns', () => {
    render(() => <App />);
    const splitter = screen.getByTestId('splitter-left');
    expect(splitter).toBeTruthy();
    expect(splitter.dataset.orientation).toBe('vertical');
  });

  it('has a vertical splitter between viewport and side panel columns', () => {
    render(() => <App />);
    const splitter = screen.getByTestId('splitter-right');
    expect(splitter).toBeTruthy();
    expect(splitter.dataset.orientation).toBe('vertical');
  });

  it('dragging left splitter updates main grid columns', () => {
    render(() => <App />);
    const splitter = screen.getByTestId('splitter-left');
    const main = screen.getByTestId('app-layout').querySelector('[class*="main"]') as HTMLElement;
    expect(main).toBeTruthy();

    // Get initial grid-template-columns
    const initialColumns = main.style.gridTemplateColumns;

    // Drag right by 50px
    fireEvent.mouseDown(splitter, { clientX: 300, clientY: 200 });
    fireEvent.mouseMove(document, { clientX: 350, clientY: 200 });
    fireEvent.mouseUp(document);

    // Grid template columns should have changed
    const updatedColumns = main.style.gridTemplateColumns;
    expect(updatedColumns).not.toBe(initialColumns);
    // Should contain 350px (300 + 50) for editor panel width
    expect(updatedColumns).toContain('350px');
  });
});

describe('App initial state loading', () => {
  it('calls getInitialState on mount and populates store values into PropertyEditor', async () => {
    const testState: GuiState = {
      meshes: [],
      values: [
        {
          cell_id: 'c1',
          name: 'width',
          value: '80',
          unit: 'mm',
          determinacy: 'determined',
          entity_path: 'Bracket.width',
        },
      ],
      constraints: [],
      files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
    };

    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);

    render(() => <App />);

    // Wait for async getInitialState to resolve and populate the store
    await waitFor(() => {
      // PropertyEditor should show the value from the initial state
      expect(screen.getByText('width')).toBeTruthy();
    });

    expect(bridge.getInitialState).toHaveBeenCalledOnce();
  });
});
