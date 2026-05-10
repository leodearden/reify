import { describe, it, expect } from 'vitest';
import {
  EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG,
  messageForSaveBlocked,
} from '../editor/messages';

describe('messages module', () => {
  it('(a) EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG is a non-empty string', () => {
    expect(typeof EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG).toBe('string');
    expect(EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG.length).toBeGreaterThan(0);
  });

  it('(b) messageForSaveBlocked("externally-changed") returns EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG', () => {
    expect(messageForSaveBlocked('externally-changed')).toBe(
      EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG,
    );
  });

  it('(c) messageForSaveBlocked("not-found") returns a distinct non-empty string', () => {
    const msg = messageForSaveBlocked('not-found');
    expect(typeof msg).toBe('string');
    expect(msg.length).toBeGreaterThan(0);
    expect(msg).not.toBe(EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG);
  });
});
