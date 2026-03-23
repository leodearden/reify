import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { ChatPanel } from '../panels/ChatPanel';
import { ConstraintPanel } from '../panels/ConstraintPanel';
import { StatusBar } from '../panels/StatusBar';
import { createClaudeStore } from '../stores/claudeStore';
import type { ConstraintData, ValueData, EvaluationStatus } from '../types';

function makeStore(overrides?: { onSend?: ReturnType<typeof vi.fn>; onAbort?: ReturnType<typeof vi.fn> }) {
  return createClaudeStore({
    onSend: overrides?.onSend ?? vi.fn(),
    onAbort: overrides?.onAbort ?? vi.fn(),
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
  };
}

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
