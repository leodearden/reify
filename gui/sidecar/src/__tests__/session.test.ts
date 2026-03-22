import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { OutboundMessage } from '../types.js';

// Mock the claude SDK subprocess spawning
vi.mock('node:child_process', () => ({
  spawn: vi.fn(),
}));

import { SidecarSession } from '../session.js';

describe('SidecarSession', () => {
  let session: SidecarSession;
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    session = new SidecarSession({
      model: 'claude-sonnet-4-20250514',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are a test assistant.',
    });
    session.onOutput = (msg) => outputs.push(msg);
  });

  it('constructor creates session with config', () => {
    expect(session).toBeDefined();
    expect(session).toBeInstanceOf(SidecarSession);
  });

  it('init() calls onOutput with ready message', async () => {
    await session.init();
    expect(outputs).toHaveLength(1);
    expect(outputs[0]).toEqual({ type: 'ready' });
  });

  it('handleMessage with send_message eventually emits done message', async () => {
    await session.init();
    outputs.length = 0;

    // Mock the internal SDK call to resolve immediately
    const mockInvoke = vi.spyOn(session as any, 'invokeSdk').mockResolvedValue('Test response');

    await session.handleMessage({
      type: 'send_message',
      id: 'msg-1',
      text: 'Hello',
    });

    // Should emit at least a text_delta and done
    const types = outputs.map((o) => o.type);
    expect(types).toContain('text_delta');
    expect(types[types.length - 1]).toBe('done');

    mockInvoke.mockRestore();
  });

  it('handleMessage with abort sets abort flag', async () => {
    await session.init();

    // Start a long-running message
    const mockInvoke = vi.spyOn(session as any, 'invokeSdk').mockImplementation(
      () => new Promise((resolve) => setTimeout(resolve, 10000))
    );

    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'msg-2',
      text: 'Long task',
    });

    // Abort immediately
    await session.handleMessage({ type: 'abort' });

    // The abort should cause the SDK call to be cancelled
    expect((session as any).abortController?.signal.aborted).toBe(true);

    mockInvoke.mockRestore();
  });

  it('handleMessage with clear_session resets history and emits ready', async () => {
    await session.init();
    outputs.length = 0;

    // Add some conversation history
    const mockInvoke = vi.spyOn(session as any, 'invokeSdk').mockResolvedValue('Response');
    await session.handleMessage({
      type: 'send_message',
      id: 'msg-3',
      text: 'First message',
    });

    outputs.length = 0;

    await session.handleMessage({ type: 'clear_session' });

    expect(outputs).toHaveLength(1);
    expect(outputs[0]).toEqual({ type: 'ready' });
    expect((session as any).conversationHistory).toHaveLength(0);

    mockInvoke.mockRestore();
  });

  it('SDK errors produce error outbound message', async () => {
    await session.init();
    outputs.length = 0;

    const mockInvoke = vi.spyOn(session as any, 'invokeSdk').mockRejectedValue(
      new Error('Authentication failed: invalid API key')
    );

    await session.handleMessage({
      type: 'send_message',
      id: 'msg-4',
      text: 'Hello',
    });

    const errorMsgs = outputs.filter((o) => o.type === 'error');
    expect(errorMsgs).toHaveLength(1);
    expect((errorMsgs[0] as any).message).toContain('Authentication failed');

    mockInvoke.mockRestore();
  });

  it('multiple sequential messages maintain conversation context', async () => {
    await session.init();
    outputs.length = 0;

    const mockInvoke = vi.spyOn(session as any, 'invokeSdk');
    mockInvoke.mockResolvedValueOnce('First response');
    mockInvoke.mockResolvedValueOnce('Second response');

    await session.handleMessage({
      type: 'send_message',
      id: 'msg-5',
      text: 'Message one',
    });

    await session.handleMessage({
      type: 'send_message',
      id: 'msg-6',
      text: 'Message two',
    });

    // Should have accumulated conversation history
    const history = (session as any).conversationHistory;
    expect(history.length).toBeGreaterThanOrEqual(4); // 2 user msgs + 2 assistant msgs

    mockInvoke.mockRestore();
  });
});
