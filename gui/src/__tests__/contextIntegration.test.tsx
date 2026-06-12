import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@solidjs/testing-library';

// Mock Tauri APIs before any component imports
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] }),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

// Mock Viewport + DualViewport
vi.mock('../viewport', () => ({
  Viewport: (props: any) => {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'viewport-container');
    if (props.fitToViewRef) props.fitToViewRef(() => {});
    if (props.flyToEntityRef) props.flyToEntityRef(() => {});
    return el;
  },
  DualViewport: (props: any) => {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'dual-viewport');
    if (props.fitToViewRef) props.fitToViewRef(() => {});
    if (props.flyToEntityRef) props.flyToEntityRef(() => {});
    return el;
  },
}));

// Mock Editor
vi.mock('../editor/Editor', () => ({
  Editor: () => {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'editor-container');
    return el;
  },
}));

// Mock FileTabs
vi.mock('../editor/FileTabs', () => ({
  FileTabs: () => {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'file-tabs');
    return el;
  },
}));

// Mock bridge functions
vi.mock('../bridge', () => ({
  getInitialState: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] }),
  getEntityTree: vi.fn().mockResolvedValue([]),
  setParameter: vi.fn().mockResolvedValue(undefined),
  exportGeometry: vi.fn().mockResolvedValue(undefined),
  pickSavePath: vi.fn().mockResolvedValue('/path.step'),
  pickOpenPath: vi.fn().mockResolvedValue(null),
  updateSource: vi.fn().mockResolvedValue(undefined),
  openFile: vi.fn().mockResolvedValue({ path: '', content: '' }),
  openFileEngine: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] }),
  getSourceLocation: vi.fn().mockResolvedValue({ file_path: '/test.ri', line: 1, column: 1, end_line: 1, end_column: 5 }),
  focusEntity: vi.fn().mockResolvedValue(undefined),
  onMeshUpdate: vi.fn().mockResolvedValue(() => {}),
  onValueUpdate: vi.fn().mockResolvedValue(() => {}),
  onConstraintUpdate: vi.fn().mockResolvedValue(() => {}),
  onEvaluationStatus: vi.fn().mockResolvedValue(() => {}),
  onMeshRemoved: vi.fn().mockResolvedValue(() => {}),
  onValueRemoved: vi.fn().mockResolvedValue(() => {}),
  onConstraintRemoved: vi.fn().mockResolvedValue(() => {}),
  onFileChanged: vi.fn().mockResolvedValue(() => {}),
  isDebugEnabled: vi.fn().mockResolvedValue(false),
  getKernelStatus: vi.fn().mockResolvedValue({ available: true, message: null }),
  onKernelStatus: vi.fn().mockResolvedValue(() => {}),
  getContainingDefinition: vi.fn().mockResolvedValue(null),
  getEntityAtSourceLocation: vi.fn().mockResolvedValue(null),
  getDefPreview: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [], compile_diagnostics: [], tensegrity_wires: [], tensegrity_surfaces: [] }),
  getMechanismDescriptors: vi.fn().mockResolvedValue([]),
}));

import { ChatPanel } from '../panels/ChatPanel';
import { ConstraintPanel } from '../panels/ConstraintPanel';
import { StatusBar } from '../panels/StatusBar';
import App from '../App';
import { createClaudeStore } from '../stores/claudeStore';
import * as bridge from '../bridge';
import type { ConstraintData, ValueData, EvaluationStatus } from '../types';

function makeStore(overrides?: { onSend?: ReturnType<typeof vi.fn>; onAbort?: ReturnType<typeof vi.fn> }) {
  return createClaudeStore({
    onSend: overrides?.onSend ?? vi.fn(),
    onAbort: overrides?.onAbort ?? vi.fn(),
    onPermissionDecision: vi.fn(),
  });
}

function makeConstraint(overrides: Partial<ConstraintData> & { node_id: string }): ConstraintData {
  return {
    node_id: overrides.node_id,
    expression: overrides.expression ?? 'x > 0',
    status: overrides.status ?? 'satisfied',
    label: overrides.label ?? null,
    parameter_ids: overrides.parameter_ids ?? [],
  };
}

function makeValue(overrides: Partial<ValueData> & { cell_id: string }): ValueData {
  return {
    cell_id: overrides.cell_id,
    name: overrides.name ?? 'param',
    value: overrides.value ?? '10',
    unit: overrides.unit ?? 'mm',
    determinacy: overrides.determinacy ?? 'determined',
    entity_path: overrides.entity_path ?? 'Bracket.param',
    kind: overrides.kind ?? 'Param',
    freshness: overrides.freshness ?? 'final',
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  // Reset bridge mock implementations
  vi.mocked(bridge.getInitialState).mockResolvedValue({ meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [], compile_diagnostics: [], tensegrity_wires: [], tensegrity_surfaces: [] });
  vi.mocked(bridge.onMeshUpdate).mockResolvedValue(() => {});
  vi.mocked(bridge.onValueUpdate).mockResolvedValue(() => {});
  vi.mocked(bridge.onConstraintUpdate).mockResolvedValue(() => {});
  vi.mocked(bridge.onEvaluationStatus).mockResolvedValue(() => {});
  vi.mocked(bridge.onMeshRemoved).mockResolvedValue(() => {});
  vi.mocked(bridge.onValueRemoved).mockResolvedValue(() => {});
  vi.mocked(bridge.onConstraintRemoved).mockResolvedValue(() => {});
  vi.mocked(bridge.onFileChanged).mockResolvedValue(() => {});
  vi.mocked(bridge.getEntityTree).mockResolvedValue([]);
});

afterEach(() => {
  cleanup();
});

describe('Context integration', () => {
  it('ChatPanel with violated constraints enables "Violated constraints" context picker option', () => {
    const store = makeStore();
    const constraints = [
      { expression: 'width > 100', status: 'violated' },
      { expression: 'height > 0', status: 'satisfied' },
    ];
    render(() => (
      <ChatPanel
        store={store}
        engineConstraints={constraints}
        diagnostics={[]}
      />
    ));
    fireEvent.click(screen.getByTestId('context-picker-btn'));
    const violatedOption = screen.getByText('Violated constraints');
    expect((violatedOption as HTMLButtonElement).disabled).toBe(false);
  });

  it('attaching "Violated constraints" and sending includes constraints in context', () => {
    const onSend = vi.fn();
    const store = makeStore({ onSend });
    const constraints = [
      { expression: 'width > 100', status: 'violated' },
    ];
    render(() => (
      <ChatPanel
        store={store}
        engineConstraints={constraints}
        diagnostics={[]}
      />
    ));
    fireEvent.click(screen.getByTestId('context-picker-btn'));
    fireEvent.click(screen.getByText('Violated constraints'));
    const textarea = screen.getByTestId('chat-input') as HTMLTextAreaElement;
    fireEvent.input(textarea, { target: { value: 'Fix this' } });
    fireEvent.click(screen.getByTestId('send-button'));
    expect(onSend).toHaveBeenCalledWith(
      expect.any(String),
      'Fix this',
      expect.objectContaining({
        constraints: expect.arrayContaining(['width > 100']),
      }),
    );
  });

  it('ConstraintPanel onAskClaude provides constraint context string', () => {
    const onAskClaude = vi.fn();
    const constraint = makeConstraint({
      node_id: 'n1',
      status: 'violated',
      expression: 'width > 100',
      parameter_ids: ['c1'],
    });
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', value: '50' }),
    };
    render(() => (
      <ConstraintPanel
        constraints={{ n1: constraint }}
        values={values}
        onAskClaude={onAskClaude}
      />
    ));
    const row = screen.getByTestId('constraint-row-n1');
    fireEvent.contextMenu(row);
    fireEvent.click(screen.getByText('Ask Claude about this constraint'));
    expect(onAskClaude).toHaveBeenCalledTimes(1);
    const ctx = onAskClaude.mock.calls[0][0] as string;
    expect(ctx).toContain('Constraint: width > 100');
    expect(ctx).toContain('Status: violated');
    expect(ctx).toContain('width=50');
  });

  it('StatusBar with claudeStatus="thinking" displays correct text', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' } as EvaluationStatus}
        meshes={{}}
        constraints={{}}
        claudeStatus="thinking"
      />
    ));
    const el = screen.getByTestId('claude-status');
    expect(el.textContent).toContain('thinking...');
  });

  it('error outbound message adds system message to chat', () => {
    const store = makeStore();
    store.sendMessage('Hello', {});
    const msgId = store.state.currentMessageId!;
    store.handleOutboundMessage({ type: 'error', id: msgId, message: 'Unauthorized: 401' });
    render(() => <ChatPanel store={store} />);
    expect(screen.getByTestId('system-message')).toBeTruthy();
    expect(screen.getByTestId('system-message').textContent).toContain('Authentication required');
  });
});

/** Helper: render App and wait for init to complete (ready state). */
async function renderAndWaitForReady() {
  const result = render(() => <App />);
  await waitFor(() => {
    expect(screen.getByTestId('app-layout')).toBeTruthy();
  });
  return result;
}

describe('App wiring', () => {
  it('StatusBar receives claudeStatus from claudeStore sessionStatus', async () => {
    await renderAndWaitForReady();
    // The StatusBar should have a claude-status section showing the claude store's idle status
    expect(screen.getByTestId('claude-status')).toBeTruthy();
    expect(screen.getByTestId('claude-status').textContent).toContain('idle');
  });

  it('ConstraintPanel receives onAskClaude handler — right-clicking shows context menu', async () => {
    // Provide initial state with a constraint
    vi.mocked(bridge.getInitialState).mockResolvedValue({
      meshes: [],
      values: [],
      constraints: [
        { node_id: 'c1', expression: 'width > 100', status: 'violated', label: null, parameter_ids: [] },
      ],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
      tensegrity_surfaces: [],
    });
    await renderAndWaitForReady();
    const row = screen.getByTestId('constraint-row-c1');
    fireEvent.contextMenu(row);
    expect(screen.getByTestId('constraint-context-menu')).toBeTruthy();
  });

  it('ChatPanel receives engineConstraints from engineStore', async () => {
    // Provide initial state with a violated constraint
    vi.mocked(bridge.getInitialState).mockResolvedValue({
      meshes: [],
      values: [],
      constraints: [
        { node_id: 'c1', expression: 'width > 100', status: 'violated', label: null, parameter_ids: [] },
      ],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
      tensegrity_surfaces: [],
    });
    await renderAndWaitForReady();
    // Open context picker — 'Violated constraints' should be enabled
    fireEvent.click(screen.getByTestId('context-picker-btn'));
    const option = screen.getByText('Violated constraints');
    expect((option as HTMLButtonElement).disabled).toBe(false);
  });

  it('StatusBar onToggleChat toggles ChatPanel visibility', async () => {
    await renderAndWaitForReady();
    // ChatPanel should be visible initially
    expect(screen.getByTestId('chat-panel')).toBeTruthy();
    // Click the Claude status indicator to toggle
    fireEvent.click(screen.getByTestId('claude-status'));
    // ChatPanel should be hidden
    expect(screen.queryByTestId('chat-panel')).toBeNull();
    // Click again to show
    fireEvent.click(screen.getByTestId('claude-status'));
    expect(screen.getByTestId('chat-panel')).toBeTruthy();
  });
});
