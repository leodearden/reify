import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { MessageGroup } from '../panels/chat/MessageGroup';
import { ChatPanel } from '../panels/ChatPanel';
import { createClaudeStore } from '../stores/claudeStore';
import type { AssistantMessage, ClaudeState, ChatMessage, UserMessage } from '../stores/claudeStore';

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

function makeStore(overrides?: { onSend?: ReturnType<typeof vi.fn>; onAbort?: ReturnType<typeof vi.fn> }) {
  return createClaudeStore({
    onSend: overrides?.onSend ?? vi.fn(),
    onAbort: overrides?.onAbort ?? vi.fn(),
  });
}

describe('ChatPanel', () => {
  it('renders with data-testid="chat-panel"', () => {
    const store = makeStore();
    render(() => <ChatPanel store={store} />);
    expect(screen.getByTestId('chat-panel')).toBeTruthy();
  });

  it('shows empty state message when no messages', () => {
    const store = makeStore();
    render(() => <ChatPanel store={store} />);
    expect(screen.getByTestId('chat-panel').textContent).toContain('Start a conversation');
  });

  it('renders user message with data-testid="user-message"', () => {
    const store = makeStore();
    store.sendMessage('Hello world', {});
    render(() => <ChatPanel store={store} />);
    expect(screen.getByTestId('user-message')).toBeTruthy();
    expect(screen.getByTestId('user-message').textContent).toContain('Hello world');
  });

  it('renders assistant message via MessageGroup', () => {
    const store = makeStore();
    store.sendMessage('Hello', {});
    // Feed a text delta to give the assistant some response text
    const msgId = store.state.currentMessageId!;
    store.handleOutboundMessage({ type: 'text_delta', id: msgId, content: 'Hi there!' });
    store.handleOutboundMessage({ type: 'done', id: msgId });
    render(() => <ChatPanel store={store} />);
    expect(screen.getByTestId('message-group')).toBeTruthy();
    expect(screen.getByTestId('streaming-text').textContent).toContain('Hi there!');
  });

  it('abort button visible when sessionStatus is responding', () => {
    const store = makeStore();
    store.sendMessage('Hello', {});
    const msgId = store.state.currentMessageId!;
    store.handleOutboundMessage({ type: 'text_delta', id: msgId, content: 'Hi' });
    // sessionStatus should now be 'responding'
    render(() => <ChatPanel store={store} />);
    expect(screen.getByTestId('abort-button')).toBeTruthy();
  });

  it('abort button NOT visible when sessionStatus is idle', () => {
    const store = makeStore();
    render(() => <ChatPanel store={store} />);
    expect(screen.queryByTestId('abort-button')).toBeNull();
  });

  it('send button visible when sessionStatus is idle', () => {
    const store = makeStore();
    render(() => <ChatPanel store={store} />);
    expect(screen.getByTestId('send-button')).toBeTruthy();
  });

  it('send button disabled when textarea is empty', () => {
    const store = makeStore();
    render(() => <ChatPanel store={store} />);
    const sendBtn = screen.getByTestId('send-button') as HTMLButtonElement;
    expect(sendBtn.disabled).toBe(true);
  });

  it('typing in textarea and clicking send calls onSend', async () => {
    const onSend = vi.fn();
    const store = makeStore({ onSend });
    render(() => <ChatPanel store={store} />);
    const textarea = screen.getByTestId('chat-input') as HTMLTextAreaElement;
    fireEvent.input(textarea, { target: { value: 'Build a box' } });
    const sendBtn = screen.getByTestId('send-button') as HTMLButtonElement;
    fireEvent.click(sendBtn);
    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledWith(expect.any(String), 'Build a box', {});
  });

  it('full event sequence: sendMessage, text_deltas, done → rendered output', () => {
    const store = makeStore();
    store.sendMessage('Make a sphere', {});
    const msgId = store.state.currentMessageId!;
    store.handleOutboundMessage({ type: 'thinking_delta', id: msgId, content: 'Let me think...' });
    store.handleOutboundMessage({ type: 'text_delta', id: msgId, content: 'Here is ' });
    store.handleOutboundMessage({ type: 'text_delta', id: msgId, content: 'a sphere.' });
    store.handleOutboundMessage({ type: 'done', id: msgId });

    render(() => <ChatPanel store={store} />);

    // User message should be rendered
    expect(screen.getByTestId('user-message').textContent).toContain('Make a sphere');
    // Assistant message should have combined response
    expect(screen.getByTestId('streaming-text').textContent).toContain('Here is a sphere.');
    // Thinking block should appear (collapsed, since complete)
    expect(screen.getByTestId('thinking-block')).toBeTruthy();
    // Session should be idle, so no abort button
    expect(screen.queryByTestId('abort-button')).toBeNull();
  });
});
