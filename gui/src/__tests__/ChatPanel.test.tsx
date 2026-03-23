// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { ChatPanel } from '../panels/ChatPanel';
import type { ChatMessage } from '../types';

const defaultProps = () => ({
  messages: [] as ChatMessage[],
  sessionStatus: 'idle' as const,
  onSendMessage: vi.fn(),
  onClearSession: vi.fn(),
  onToggle: vi.fn(),
  open: true,
  height: 250,
  onResize: vi.fn(),
});

describe('ChatPanel basic rendering', () => {
  it('renders with data-testid="chat-panel"', () => {
    render(() => <ChatPanel {...defaultProps()} />);
    expect(screen.getByTestId('chat-panel')).toBeTruthy();
  });

  it('shows header with "Claude Session" title text', () => {
    render(() => <ChatPanel {...defaultProps()} />);
    expect(screen.getByText('Claude Session')).toBeTruthy();
  });

  it('shows empty state message when messages array is empty', () => {
    render(() => <ChatPanel {...defaultProps()} />);
    expect(
      screen.getByText('Start a conversation with Claude to get help with your design.'),
    ).toBeTruthy();
  });

  it('empty state is NOT shown when messages array has items', () => {
    const messages: ChatMessage[] = [
      { id: '1', role: 'user', content: 'Hello', timestamp: 1 },
    ];
    render(() => <ChatPanel {...defaultProps()} messages={messages} />);
    expect(
      screen.queryByText('Start a conversation with Claude to get help with your design.'),
    ).toBeNull();
  });
});

describe('ChatPanel header buttons', () => {
  it('minimize button calls onToggle on click', () => {
    const onToggle = vi.fn();
    render(() => <ChatPanel {...defaultProps()} onToggle={onToggle} />);
    fireEvent.click(screen.getByTestId('chat-minimize-btn'));
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it('close button calls onToggle on click', () => {
    const onToggle = vi.fn();
    render(() => <ChatPanel {...defaultProps()} onToggle={onToggle} />);
    fireEvent.click(screen.getByTestId('chat-close-btn'));
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it('clear session button calls onClearSession on click', () => {
    const onClearSession = vi.fn();
    render(() => <ChatPanel {...defaultProps()} onClearSession={onClearSession} />);
    fireEvent.click(screen.getByTestId('chat-clear-btn'));
    expect(onClearSession).toHaveBeenCalledTimes(1);
  });
});

describe('ChatPanel message list rendering', () => {
  const testMessages: ChatMessage[] = [
    { id: '1', role: 'user', content: 'Hello', timestamp: 1 },
    { id: '2', role: 'assistant', content: 'Hi there', timestamp: 2 },
  ];

  it('user messages render with data-role="user"', () => {
    render(() => <ChatPanel {...defaultProps()} messages={testMessages} />);
    const msg = screen.getByTestId('chat-message-1');
    expect(msg.getAttribute('data-role')).toBe('user');
  });

  it('assistant messages render with data-role="assistant"', () => {
    render(() => <ChatPanel {...defaultProps()} messages={testMessages} />);
    const msg = screen.getByTestId('chat-message-2');
    expect(msg.getAttribute('data-role')).toBe('assistant');
  });

  it('message content text is visible', () => {
    render(() => <ChatPanel {...defaultProps()} messages={testMessages} />);
    expect(screen.getByText('Hello')).toBeTruthy();
    expect(screen.getByText('Hi there')).toBeTruthy();
  });

  it('correct number of message elements rendered', () => {
    const { container } = render(() => <ChatPanel {...defaultProps()} messages={testMessages} />);
    const messageEls = container.querySelectorAll('[data-role]');
    expect(messageEls.length).toBe(testMessages.length);
  });
});

describe('ChatPanel input bar', () => {
  it('textarea renders with placeholder', () => {
    render(() => <ChatPanel {...defaultProps()} />);
    const textarea = screen.getByTestId('chat-input') as HTMLTextAreaElement;
    expect(textarea.placeholder).toBe('Ask Claude about your design...');
  });

  it('typing and pressing Enter calls onSendMessage with text', () => {
    const onSendMessage = vi.fn();
    render(() => <ChatPanel {...defaultProps()} onSendMessage={onSendMessage} />);
    const textarea = screen.getByTestId('chat-input') as HTMLTextAreaElement;
    fireEvent.input(textarea, { target: { value: 'Hello Claude' } });
    fireEvent.keyDown(textarea, { key: 'Enter', shiftKey: false });
    expect(onSendMessage).toHaveBeenCalledWith('Hello Claude');
  });

  it('input is cleared after sending', () => {
    const onSendMessage = vi.fn();
    render(() => <ChatPanel {...defaultProps()} onSendMessage={onSendMessage} />);
    const textarea = screen.getByTestId('chat-input') as HTMLTextAreaElement;
    fireEvent.input(textarea, { target: { value: 'Hello Claude' } });
    fireEvent.keyDown(textarea, { key: 'Enter', shiftKey: false });
    expect(textarea.value).toBe('');
  });

  it('Shift+Enter does NOT call onSendMessage', () => {
    const onSendMessage = vi.fn();
    render(() => <ChatPanel {...defaultProps()} onSendMessage={onSendMessage} />);
    const textarea = screen.getByTestId('chat-input') as HTMLTextAreaElement;
    fireEvent.input(textarea, { target: { value: 'Hello Claude' } });
    fireEvent.keyDown(textarea, { key: 'Enter', shiftKey: true });
    expect(onSendMessage).not.toHaveBeenCalled();
  });

  it('Enter on empty input does NOT call onSendMessage', () => {
    const onSendMessage = vi.fn();
    render(() => <ChatPanel {...defaultProps()} onSendMessage={onSendMessage} />);
    const textarea = screen.getByTestId('chat-input') as HTMLTextAreaElement;
    fireEvent.keyDown(textarea, { key: 'Enter', shiftKey: false });
    expect(onSendMessage).not.toHaveBeenCalled();
  });

  it('send button click calls onSendMessage with current input text', () => {
    const onSendMessage = vi.fn();
    render(() => <ChatPanel {...defaultProps()} onSendMessage={onSendMessage} />);
    const textarea = screen.getByTestId('chat-input') as HTMLTextAreaElement;
    fireEvent.input(textarea, { target: { value: 'Hi!' } });
    fireEvent.click(screen.getByTestId('chat-send-btn'));
    expect(onSendMessage).toHaveBeenCalledWith('Hi!');
  });
});

describe('ChatPanel auto-scroll', () => {
  it('scrolls to bottom when new messages are added and user is near bottom', async () => {
    const initialMessages: ChatMessage[] = Array.from({ length: 20 }, (_, i) => ({
      id: String(i),
      role: 'user' as const,
      content: `Message ${i} with enough text to take up space`,
      timestamp: i,
    }));

    const [messages, setMessages] = createSignal<ChatMessage[]>(initialMessages);

    render(() => (
      <ChatPanel
        messages={messages()}
        sessionStatus="idle"
        onSendMessage={vi.fn()}
        onClearSession={vi.fn()}
        onToggle={vi.fn()}
        open={true}
        height={150}
        onResize={vi.fn()}
      />
    ));

    const messageList = screen.getByTestId('chat-message-list');
    // Mock layout properties to simulate overflow
    Object.defineProperty(messageList, 'scrollHeight', { value: 1000, configurable: true });
    Object.defineProperty(messageList, 'clientHeight', { value: 150, configurable: true });
    // Start near bottom so auto-scroll triggers (850 + 150 = 1000 >= 950 = 1000 - 50)
    messageList.scrollTop = 850;

    setMessages([
      ...initialMessages,
      { id: '20', role: 'assistant', content: 'New message!', timestamp: 20 },
    ]);

    // Wait for effect to run
    await new Promise((r) => setTimeout(r, 50));

    // Without auto-scroll implementation, scrollTop stays at 100
    // With auto-scroll, it should be set to scrollHeight (1000)
    expect(messageList.scrollTop).toBe(1000);
  });
});

describe('ChatPanel open/close toggle', () => {
  it('when open=false, message list and input bar are NOT in the DOM', () => {
    render(() => <ChatPanel {...defaultProps()} open={false} />);
    expect(screen.queryByTestId('chat-message-list')).toBeNull();
    expect(screen.queryByTestId('chat-input')).toBeNull();
  });

  it('when open=true, message list and input bar ARE in the DOM', () => {
    const messages: ChatMessage[] = [
      { id: '1', role: 'user', content: 'Hello', timestamp: 1 },
    ];
    render(() => <ChatPanel {...defaultProps()} messages={messages} open={true} />);
    expect(screen.getByTestId('chat-message-list')).toBeTruthy();
    expect(screen.getByTestId('chat-input')).toBeTruthy();
  });

  it('resize handle is present when open=true', () => {
    const messages: ChatMessage[] = [
      { id: '1', role: 'user', content: 'Hello', timestamp: 1 },
    ];
    render(() => <ChatPanel {...defaultProps()} messages={messages} open={true} />);
    expect(screen.getByTestId('chat-resize-handle')).toBeTruthy();
  });
});

describe('ChatPanel drag-to-resize', () => {
  it('Splitter with orientation="horizontal" and data-testid="chat-resize-handle" exists when open', () => {
    render(() => <ChatPanel {...defaultProps()} open={true} />);
    const handle = screen.getByTestId('chat-resize-handle');
    expect(handle.getAttribute('aria-orientation')).toBe('horizontal');
  });

  it('mouseDown + mouseMove on splitter triggers onResize with negated delta', () => {
    const onResize = vi.fn();
    render(() => <ChatPanel {...defaultProps()} open={true} onResize={onResize} />);
    const handle = screen.getByTestId('chat-resize-handle');

    // Start drag at y=300
    fireEvent.mouseDown(handle, { clientX: 100, clientY: 300 });
    // Move to y=280 (delta = -20 from Splitter, negated = +20 to onResize)
    fireEvent.mouseMove(document, { clientX: 100, clientY: 280 });

    expect(onResize).toHaveBeenCalledWith(20);

    // Release
    fireEvent.mouseUp(document);
  });
});

describe('ChatPanel disabled state', () => {
  it('textarea is disabled when sessionStatus is busy', () => {
    render(() => <ChatPanel {...defaultProps()} sessionStatus="busy" />);
    const textarea = screen.getByTestId('chat-input') as HTMLTextAreaElement;
    expect(textarea.disabled).toBe(true);
  });

  it('send button is disabled when sessionStatus is busy', () => {
    render(() => <ChatPanel {...defaultProps()} sessionStatus="busy" />);
    const btn = screen.getByTestId('chat-send-btn') as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
  });

  it('textarea is NOT disabled when sessionStatus is idle', () => {
    render(() => <ChatPanel {...defaultProps()} sessionStatus="idle" />);
    const textarea = screen.getByTestId('chat-input') as HTMLTextAreaElement;
    expect(textarea.disabled).toBe(false);
  });

  it('send button is NOT disabled when sessionStatus is idle', () => {
    render(() => <ChatPanel {...defaultProps()} sessionStatus="idle" />);
    const btn = screen.getByTestId('chat-send-btn') as HTMLButtonElement;
    expect(btn.disabled).toBe(false);
  });
});
