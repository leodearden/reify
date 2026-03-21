import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';

// Mock Tauri APIs before any component imports
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] }),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

// Mock Viewport (requires Three.js / WebGL which jsdom doesn't support)
vi.mock('../viewport', () => ({
  Viewport: (props: any) => <div data-testid="viewport-container">Viewport Mock</div>,
}));

// Mock Editor (requires CodeMirror DOM APIs)
vi.mock('../editor/Editor', () => ({
  Editor: (props: any) => <div data-testid="editor-container">Editor Mock</div>,
}));

// Mock FileTabs
vi.mock('../editor/FileTabs', () => ({
  FileTabs: (props: any) => <div data-testid="file-tabs">FileTabs Mock</div>,
}));

import App from '../App';

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  vi.restoreAllMocks();
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
