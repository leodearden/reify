import { describe, it, expect } from 'vitest';
import {
  EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG,
  messageForSaveBlocked,
} from '../editor/messages';

describe('messages module', () => {
  it('messageForSaveBlocked("externally-changed") returns EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG', () => {
    expect(messageForSaveBlocked('externally-changed')).toBe(
      EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG,
    );
  });
});
