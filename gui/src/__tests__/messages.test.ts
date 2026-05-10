import { describe, it, expect } from 'vitest';
import {
  EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG,
  FILE_NOT_OPEN_SAVE_BLOCKED_MSG,
  messageForSaveBlocked,
} from '../editor/messages';

describe('messages module', () => {
  it('messageForSaveBlocked("externally-changed") returns EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG', () => {
    expect(messageForSaveBlocked('externally-changed')).toBe(
      EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG,
    );
  });

  it('messageForSaveBlocked("not-found") returns FILE_NOT_OPEN_SAVE_BLOCKED_MSG', () => {
    expect(messageForSaveBlocked('not-found')).toBe(
      FILE_NOT_OPEN_SAVE_BLOCKED_MSG,
    );
  });
});
