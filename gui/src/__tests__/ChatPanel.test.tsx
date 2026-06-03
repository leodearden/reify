import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { MessageGroup } from '../panels/chat/MessageGroup';
import { ChatPanel } from '../panels/ChatPanel';
import { createClaudeStore } from '../stores/claudeStore';
import type { AssistantMessage, ClaudeState, ChatMessage, UserMessage, PermissionDecision } from '../stores/claudeStore';

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
    onPermissionDecision: vi.fn(),
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

  it('abort button visible when sessionStatus is thinking', () => {
    const store = makeStore();
    store.sendMessage('Hello', {});
    const msgId = store.state.currentMessageId!;
    store.handleOutboundMessage({ type: 'thinking_delta', id: msgId, content: 'hmm' });
    // sessionStatus should now be 'thinking'
    render(() => <ChatPanel store={store} />);
    expect(screen.getByTestId('abort-button')).toBeTruthy();
  });

  it('abort button visible when sessionStatus is tool-calling', () => {
    const store = makeStore();
    store.sendMessage('Hello', {});
    const msgId = store.state.currentMessageId!;
    store.handleOutboundMessage({ type: 'tool_call', id: msgId, tool_use_id: 'tuid-1', tool_name: 'reify_get_parameters', tool_input: {} });
    // sessionStatus should now be 'tool-calling'
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

  describe('context integration', () => {
    it('renders context picker button in input area', () => {
      const store = makeStore();
      render(() => <ChatPanel store={store} />);
      expect(screen.getByTestId('context-picker-btn')).toBeTruthy();
    });

    it('renders context chips area when items are attached', () => {
      const store = makeStore();
      render(() => (
        <ChatPanel
          store={store}
          selectedEntity="box1"
          engineConstraints={[]}
          diagnostics={[]}
        />
      ));
      // Open picker and attach selection
      fireEvent.click(screen.getByTestId('context-picker-btn'));
      fireEvent.click(screen.getByText('Current selection'));
      expect(screen.getByTestId('context-chips')).toBeTruthy();
    });

    it('attaching selection context shows a ContextChip with entity label', () => {
      const store = makeStore();
      render(() => (
        <ChatPanel
          store={store}
          selectedEntity="cylinder1"
          engineConstraints={[]}
          diagnostics={[]}
        />
      ));
      fireEvent.click(screen.getByTestId('context-picker-btn'));
      fireEvent.click(screen.getByText('Current selection'));
      expect(screen.getByTestId('context-chip')).toBeTruthy();
      expect(screen.getByTestId('context-chip').textContent).toContain('cylinder1');
    });

    it('removing a chip removes it from display', () => {
      const store = makeStore();
      render(() => (
        <ChatPanel
          store={store}
          selectedEntity="box1"
          engineConstraints={[]}
          diagnostics={[]}
        />
      ));
      fireEvent.click(screen.getByTestId('context-picker-btn'));
      fireEvent.click(screen.getByText('Current selection'));
      expect(screen.getByTestId('context-chip')).toBeTruthy();
      fireEvent.click(screen.getByTestId('chip-remove'));
      expect(screen.queryByTestId('context-chip')).toBeNull();
    });

    it('sending a message includes attached contexts in sendMessage call', () => {
      const onSend = vi.fn();
      const store = makeStore({ onSend });
      render(() => (
        <ChatPanel
          store={store}
          selectedEntity="box1"
          engineConstraints={[]}
          diagnostics={[]}
        />
      ));
      fireEvent.click(screen.getByTestId('context-picker-btn'));
      fireEvent.click(screen.getByText('Current selection'));
      const textarea = screen.getByTestId('chat-input') as HTMLTextAreaElement;
      fireEvent.input(textarea, { target: { value: 'Resize it' } });
      fireEvent.click(screen.getByTestId('send-button'));
      expect(onSend).toHaveBeenCalledWith(
        expect.any(String),
        'Resize it',
        expect.objectContaining({ selectedEntity: 'box1' })
      );
    });

    it('after sending, attached contexts are cleared', () => {
      const store = makeStore();
      render(() => (
        <ChatPanel
          store={store}
          selectedEntity="box1"
          engineConstraints={[]}
          diagnostics={[]}
        />
      ));
      fireEvent.click(screen.getByTestId('context-picker-btn'));
      fireEvent.click(screen.getByText('Current selection'));
      expect(screen.getByTestId('context-chip')).toBeTruthy();
      const textarea = screen.getByTestId('chat-input') as HTMLTextAreaElement;
      fireEvent.input(textarea, { target: { value: 'hello' } });
      fireEvent.click(screen.getByTestId('send-button'));
      expect(screen.queryByTestId('context-chip')).toBeNull();
    });

    it('system messages in store render as SystemMessage components', () => {
      const store = makeStore();
      store.addSystemMessage('auth', 'Authentication required. Run `claude login` in your terminal.');
      render(() => <ChatPanel store={store} />);
      expect(screen.getByTestId('system-message')).toBeTruthy();
    });

    it('when selectedEntity is provided, auto-context label shown on user messages', () => {
      const store = makeStore();
      store.sendMessage('Do something', {});
      render(() => (
        <ChatPanel
          store={store}
          selectedEntity="sphere1"
          engineConstraints={[]}
          diagnostics={[]}
        />
      ));
      const label = screen.queryByTestId('auto-context-label');
      expect(label).toBeTruthy();
      expect(label!.textContent).toContain('sphere1');
    });
  });

  describe('permission prompt integration', () => {
    function makePermissionStore() {
      const onPermissionDecision = vi.fn();
      const store = createClaudeStore({
        onSend: vi.fn(),
        onAbort: vi.fn(),
        onPermissionDecision,
      });
      return { store, onPermissionDecision };
    }

    function feedPermissionRequest(
      store: ReturnType<typeof createClaudeStore>,
      opts: { requestId?: string; toolName?: string; toolInput?: Record<string, unknown> } = {},
    ) {
      store.sendMessage('hello', {});
      const msgId = store.state.currentMessageId!;
      store.handleOutboundMessage({
        type: 'permission_request',
        id: msgId,
        request_id: opts.requestId ?? 'req-1',
        tool_name: opts.toolName ?? 'Write',
        tool_input: opts.toolInput ?? {},
      });
      return msgId;
    }

    it('does NOT render permission-prompts container when queue is empty', () => {
      const { store } = makePermissionStore();
      render(() => <ChatPanel store={store} />);
      expect(screen.queryByTestId('permission-prompts')).toBeNull();
    });

    it('renders permission-prompts container with one PermissionPrompt per pending request', () => {
      const { store } = makePermissionStore();
      feedPermissionRequest(store, { requestId: 'req-1', toolName: 'Write', toolInput: { path: '/tmp/x' } });
      render(() => <ChatPanel store={store} />);
      expect(screen.getByTestId('permission-prompts')).toBeTruthy();
      expect(screen.getAllByTestId('permission-prompt')).toHaveLength(1);
    });

    it('renders multiple prompts in insertion order', () => {
      const { store } = makePermissionStore();
      store.sendMessage('hello', {});
      const msgId = store.state.currentMessageId!;
      store.handleOutboundMessage({
        type: 'permission_request',
        id: msgId,
        request_id: 'req-1',
        tool_name: 'Write',
        tool_input: { path: '/tmp/a' },
      });
      store.handleOutboundMessage({
        type: 'permission_request',
        id: msgId,
        request_id: 'req-2',
        tool_name: 'Bash',
        tool_input: { command: 'ls' },
      });
      render(() => <ChatPanel store={store} />);
      const prompts = screen.getAllByTestId('permission-prompt');
      expect(prompts).toHaveLength(2);
      expect(prompts[0].textContent).toContain('Write');
      expect(prompts[1].textContent).toContain('Bash');
    });

    it('permission-prompts container appears after messages and before the input area in DOM', () => {
      const { store } = makePermissionStore();
      feedPermissionRequest(store, { requestId: 'req-1', toolName: 'Write' });
      render(() => <ChatPanel store={store} />);
      const userMsg = screen.getByTestId('user-message');
      const prompts = screen.getByTestId('permission-prompts');
      const input = screen.getByTestId('chat-input');
      // DOCUMENT_POSITION_FOLLOWING (4) means the second element follows the first
      expect(userMsg.compareDocumentPosition(prompts) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(prompts.compareDocumentPosition(input) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    });

    it('clicking Allow button calls store.decidePermission with { behavior: "allow" }', () => {
      const { store } = makePermissionStore();
      const decideSpy = vi.spyOn(store, 'decidePermission');
      feedPermissionRequest(store, { requestId: 'req-1', toolName: 'Write' });
      render(() => <ChatPanel store={store} />);
      fireEvent.click(screen.getByTestId('permission-allow'));
      expect(decideSpy).toHaveBeenCalledOnce();
      expect(decideSpy).toHaveBeenCalledWith('req-1', { behavior: 'allow' });
    });

    it('clicking Always allow calls store.decidePermission with { behavior: "allow", remember: true }', () => {
      const { store } = makePermissionStore();
      const decideSpy = vi.spyOn(store, 'decidePermission');
      feedPermissionRequest(store, { requestId: 'req-1', toolName: 'Write' });
      render(() => <ChatPanel store={store} />);
      fireEvent.click(screen.getByTestId('permission-allow-always'));
      expect(decideSpy).toHaveBeenCalledOnce();
      expect(decideSpy).toHaveBeenCalledWith('req-1', { behavior: 'allow', remember: true });
    });

    it('clicking Deny button calls store.decidePermission with { behavior: "deny" }', () => {
      const { store } = makePermissionStore();
      const decideSpy = vi.spyOn(store, 'decidePermission');
      feedPermissionRequest(store, { requestId: 'req-1', toolName: 'Write' });
      render(() => <ChatPanel store={store} />);
      fireEvent.click(screen.getByTestId('permission-deny'));
      expect(decideSpy).toHaveBeenCalledOnce();
      expect(decideSpy).toHaveBeenCalledWith('req-1', { behavior: 'deny' });
    });
  });

  describe('DOM order (panel-level)', () => {
    it('multiple messages render in store insertion order (user → assistant alternation)', () => {
      const store = makeStore();

      store.sendMessage('First user message', {});
      const msgId1 = store.state.currentMessageId!;
      store.handleOutboundMessage({ type: 'text_delta', id: msgId1, content: 'First reply' });
      store.handleOutboundMessage({ type: 'done', id: msgId1 });

      store.sendMessage('Second user message', {});
      const msgId2 = store.state.currentMessageId!;
      store.handleOutboundMessage({ type: 'text_delta', id: msgId2, content: 'Second reply' });
      store.handleOutboundMessage({ type: 'done', id: msgId2 });

      render(() => <ChatPanel store={store} />);

      const userMsgs = screen.getAllByTestId('user-message');
      const msgGroups = screen.getAllByTestId('message-group');
      expect(userMsgs).toHaveLength(2);
      expect(msgGroups).toHaveLength(2);

      // user[0] → group[0] → user[1] → group[1] in document order
      expect(userMsgs[0].compareDocumentPosition(msgGroups[0]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(msgGroups[0].compareDocumentPosition(userMsgs[1]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(userMsgs[1].compareDocumentPosition(msgGroups[1]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    });

    it('context-chips container appears between permission-prompts and chat-input in DOM', () => {
      const store = makeStore();
      store.sendMessage('hello', {});
      const msgId = store.state.currentMessageId!;
      store.handleOutboundMessage({
        type: 'permission_request',
        id: msgId,
        request_id: 'req-dom-2',
        tool_name: 'Write',
        tool_input: {},
      });

      render(() => (
        <ChatPanel
          store={store}
          selectedEntity="box1"
          engineConstraints={[]}
          diagnostics={[]}
        />
      ));

      // Attach a context chip so context-chips container appears
      fireEvent.click(screen.getByTestId('context-picker-btn'));
      fireEvent.click(screen.getByText('Current selection'));

      const permissionPrompts = screen.getByTestId('permission-prompts');
      const contextChips = screen.getByTestId('context-chips');
      const chatInput = screen.getByTestId('chat-input');

      // permission-prompts → context-chips → chat-input in document order
      expect(permissionPrompts.compareDocumentPosition(contextChips) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(contextChips.compareDocumentPosition(chatInput) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    });
  });

  describe('composer input accessibility', () => {
    it('chat-input textarea has an aria-label of "Ask Claude"', () => {
      const store = makeStore();
      render(() => <ChatPanel store={store} />);
      const textarea = screen.getByTestId('chat-input');
      expect(textarea.getAttribute('aria-label')).toBe('Ask Claude');
    });
  });
});
