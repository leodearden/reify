// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
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
    const messageEls = container.querySelectorAll('[data-testid^="chat-message-"]');
    expect(messageEls.length).toBe(testMessages.length);
  });
});
