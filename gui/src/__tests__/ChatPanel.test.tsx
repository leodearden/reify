import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
import { MessageGroup } from '../panels/chat/MessageGroup';
import type { AssistantMessage } from '../stores/claudeStore';

function makeAssistantMsg(overrides?: Partial<AssistantMessage>): AssistantMessage {
  return {
    role: 'assistant',
    id: 'msg-1',
    thinkingText: '',
    thinkingComplete: false,
    responseText: '',
    toolCalls: [],
    complete: false,
    ...overrides,
  };
}

describe('MessageGroup', () => {
  it('renders ThinkingBlock when message has non-empty thinkingText', () => {
    render(() => (
      <MessageGroup message={makeAssistantMsg({ thinkingText: 'pondering...', thinkingComplete: false })} />
    ));
    expect(screen.getByTestId('thinking-indicator')).toBeTruthy();
  });

  it('does NOT render ThinkingBlock when thinkingText is empty', () => {
    render(() => (
      <MessageGroup message={makeAssistantMsg({ thinkingText: '' })} />
    ));
    expect(screen.queryByTestId('thinking-indicator')).toBeNull();
    expect(screen.queryByTestId('thinking-block')).toBeNull();
  });

  it('renders ToolCallCard for each tool call', () => {
    const msg = makeAssistantMsg({
      toolCalls: [
        { id: 'tc-1', toolName: 'reify_get_parameters', toolInput: {}, status: 'pending' },
        { id: 'tc-2', toolName: 'reify_update_source', toolInput: {}, status: 'complete' },
      ],
    });
    render(() => <MessageGroup message={msg} />);
    const cards = screen.getAllByTestId('tool-call-card');
    expect(cards).toHaveLength(2);
  });

  it('renders StreamingText for response text', () => {
    render(() => (
      <MessageGroup message={makeAssistantMsg({ responseText: 'Hello there', complete: true })} />
    ));
    expect(screen.getByTestId('streaming-text')).toBeTruthy();
    expect(screen.getByTestId('streaming-text').textContent).toContain('Hello there');
  });

  it('visual order in DOM: thinking before tool calls before streaming text', () => {
    const msg = makeAssistantMsg({
      thinkingText: 'thinking...',
      thinkingComplete: false,
      toolCalls: [
        { id: 'tc-1', toolName: 'reify_get_parameters', toolInput: {}, status: 'pending' },
      ],
      responseText: 'response here',
    });
    render(() => <MessageGroup message={msg} />);

    const container = screen.getByTestId('streaming-text').closest('[data-testid="message-group"]')!;
    const children = container.querySelectorAll('[data-testid]');
    const testIds = Array.from(children).map((el) => el.getAttribute('data-testid'));

    const thinkingIdx = testIds.indexOf('thinking-indicator');
    const toolIdx = testIds.indexOf('tool-call-card');
    const textIdx = testIds.indexOf('streaming-text');

    expect(thinkingIdx).toBeLessThan(toolIdx);
    expect(toolIdx).toBeLessThan(textIdx);
  });
});
