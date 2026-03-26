import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock Tauri API modules (must be before imports that use them)
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import {
  claudeSendMessage,
  claudeAbort,
  claudeClearSession,
} from '../bridge';

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  vi.clearAllMocks();
});

describe('claude invoke wrappers', () => {
  it('claudeSendMessage calls invoke with command and text only', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeSendMessage('hello world');

    expect(mockInvoke).toHaveBeenCalledWith('claude_send_message', {
      text: 'hello world',
      context: undefined,
    });
  });

  it('claudeSendMessage maps camelCase context to snake_case', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeSendMessage('fix this', {
      selectedEntity: 'Box.body',
      diagnostics: ['error: type mismatch'],
      constraints: ['x > 0'],
    });

    expect(mockInvoke).toHaveBeenCalledWith('claude_send_message', {
      text: 'fix this',
      context: {
        selected_entity: 'Box.body',
        diagnostics: ['error: type mismatch'],
        constraints: ['x > 0'],
      },
    });
  });

  it('claudeSendMessage passes undefined fields correctly', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeSendMessage('hello', {
      selectedEntity: 'Bracket.w',
    });

    expect(mockInvoke).toHaveBeenCalledWith('claude_send_message', {
      text: 'hello',
      context: {
        selected_entity: 'Bracket.w',
        diagnostics: undefined,
        constraints: undefined,
      },
    });
  });

  it('claudeAbort calls invoke with correct command', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeAbort();

    expect(mockInvoke).toHaveBeenCalledWith('claude_abort');
  });

  it('claudeClearSession calls invoke with correct command', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeClearSession();

    expect(mockInvoke).toHaveBeenCalledWith('claude_clear_session');
  });
});
