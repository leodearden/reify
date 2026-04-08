import { describe, it, expect } from 'vitest';
import { errorMessage } from '../utils.js';

describe('errorMessage re-export smoke test', () => {
  it('is exported from utils', () => {
    expect(typeof errorMessage).toBe('function');
  });

  it('delegates to @reify/shared-utils implementation', () => {
    expect(errorMessage(new Error('x'))).toBe('x');
  });

  it('returns "Unknown error" for plain object with null .message', () => {
    // 'message' in err is true even when message is null (non-string),
    // so the guard catches it and returns 'Unknown error'.
    expect(errorMessage({ message: null })).toBe('Unknown error');
  });

  it('returns "Unknown error" for plain object with numeric .message', () => {
    // 'message' in err is true even when message is a number (non-string),
    // so the guard catches it and returns 'Unknown error'.
    expect(errorMessage({ message: 42 })).toBe('Unknown error');
  });

  it('returns "Unknown error" for plain object with throwing .message getter', () => {
    const hostile = { get message() { throw new Error('boom'); } };
    expect(errorMessage(hostile)).toBe('Unknown error');
  });

  it('returns "Unknown error" for Error subclass with throwing message getter', () => {
    class ThrowingError extends Error {
      get message(): string { throw new Error('boom'); }
    }
    expect(errorMessage(new ThrowingError())).toBe('Unknown error');
  });

  it('returns "Unknown error" when valueOf() and toString() both throw', () => {
    const hostile = {
      valueOf() { throw new Error('boom'); },
      toString() { throw new Error('boom'); },
    };
    expect(errorMessage(hostile)).toBe('Unknown error');
  });
});
