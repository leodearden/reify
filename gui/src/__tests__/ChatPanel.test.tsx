// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
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
