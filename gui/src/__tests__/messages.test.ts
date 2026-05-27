import { describe, it, expect } from 'vitest';
import {
  EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG,
  FILE_NOT_OPEN_SAVE_BLOCKED_MSG,
  EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG,
  SAVE_CONFLICT_RELOAD_LABEL,
  SAVE_CONFLICT_OVERWRITE_LABEL,
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

describe('save conflict prompt constants', () => {
  it('EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG is a non-empty string', () => {
    expect(typeof EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG).toBe('string');
    expect(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG.length).toBeGreaterThan(0);
  });

  it('SAVE_CONFLICT_RELOAD_LABEL is a non-empty string', () => {
    expect(typeof SAVE_CONFLICT_RELOAD_LABEL).toBe('string');
    expect(SAVE_CONFLICT_RELOAD_LABEL.length).toBeGreaterThan(0);
  });

  it('SAVE_CONFLICT_OVERWRITE_LABEL is a non-empty string', () => {
    expect(typeof SAVE_CONFLICT_OVERWRITE_LABEL).toBe('string');
    expect(SAVE_CONFLICT_OVERWRITE_LABEL.length).toBeGreaterThan(0);
  });

  it('EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG is distinct from EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG', () => {
    expect(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG).not.toBe(
      EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG,
    );
  });
});
