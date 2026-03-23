import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { ToolCallCard } from '../panels/chat/ToolCallCard';
import type { ToolCallInfo } from '../stores/claudeStore';

function makeTool(overrides?: Partial<ToolCallInfo>): ToolCallInfo {
  return {
    id: 'tc-1',
    toolName: 'reify_get_parameters',
    toolInput: { entity: 'box1' },
    status: 'pending',
    ...overrides,
  };
}

describe('ToolCallCard', () => {
  it('renders with data-testid="tool-call-card"', () => {
    render(() => <ToolCallCard toolCall={makeTool()} />);
    expect(screen.getByTestId('tool-call-card')).toBeTruthy();
  });

  it('displays tool name', () => {
    render(() => <ToolCallCard toolCall={makeTool({ toolName: 'reify_update_source' })} />);
    expect(screen.getByText('reify_update_source')).toBeTruthy();
  });

  it('shows spinner element when status="pending"', () => {
    render(() => <ToolCallCard toolCall={makeTool({ status: 'pending' })} />);
    const card = screen.getByTestId('tool-call-card');
    const spinner = card.querySelector('[data-status="pending"]');
    expect(spinner).toBeTruthy();
  });

  it('shows checkmark when status="complete"', () => {
    render(() => <ToolCallCard toolCall={makeTool({ status: 'complete' })} />);
    const card = screen.getByTestId('tool-call-card');
    expect(card.textContent).toContain('✓');
  });

  it('shows X mark when status="error"', () => {
    render(() => <ToolCallCard toolCall={makeTool({ status: 'error' })} />);
    const card = screen.getByTestId('tool-call-card');
    expect(card.textContent).toContain('✗');
  });

  it('click expands to show full input JSON in data-testid="tool-call-details"', () => {
    render(() => <ToolCallCard toolCall={makeTool({ toolInput: { entity: 'box1' } })} />);
    const header = screen.getByTestId('tool-call-card').querySelector('[role="button"]')!;
    fireEvent.click(header);
    const details = screen.getByTestId('tool-call-details');
    expect(details.textContent).toContain('entity');
    expect(details.textContent).toContain('box1');
  });

  it('for reify_update_source tool, shows "View diff" text', () => {
    render(() => <ToolCallCard toolCall={makeTool({ toolName: 'reify_update_source' })} />);
    expect(screen.getByText(/View diff/i)).toBeTruthy();
  });

  it('for reify_get_parameters with result containing array, shows count summary', () => {
    render(() => (
      <ToolCallCard
        toolCall={makeTool({
          toolName: 'reify_get_parameters',
          status: 'complete',
          result: [{ name: 'w' }, { name: 'h' }, { name: 'd' }],
        })}
      />
    ));
    expect(screen.getByText(/3 parameters/i)).toBeTruthy();
  });

  it('read tools (reify_get_*) have blue icon class', () => {
    render(() => <ToolCallCard toolCall={makeTool({ toolName: 'reify_get_source' })} />);
    const card = screen.getByTestId('tool-call-card');
    const icon = card.querySelector('[data-tool-type="read"]');
    expect(icon).toBeTruthy();
  });

  it('write tools (reify_update_*) have orange icon class', () => {
    render(() => <ToolCallCard toolCall={makeTool({ toolName: 'reify_update_source' })} />);
    const card = screen.getByTestId('tool-call-card');
    const icon = card.querySelector('[data-tool-type="write"]');
    expect(icon).toBeTruthy();
  });
});
