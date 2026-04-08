import { describe, it, expect } from 'vitest';
import { errorMessage } from '../errorMessage.js';

describe('errorMessage', () => {
  // Error instances
  it('returns .message for Error instances', () => {
    expect(errorMessage(new Error('something broke'))).toBe('something broke');
  });

  it('returns .message for Error subclass instances', () => {
    expect(errorMessage(new TypeError('bad type'))).toBe('bad type');
    expect(errorMessage(new RangeError('out of range'))).toBe('out of range');
  });

  it('returns "Unknown error" for Error with empty message', () => {
    expect(errorMessage(new Error(''))).toBe('Unknown error');
  });

  it('returns "Unknown error" for Error with whitespace-only message', () => {
    expect(errorMessage(new Error('   '))).toBe('Unknown error');
  });

  // String inputs
  it('returns the string itself for string inputs', () => {
    expect(errorMessage('plain string error')).toBe('plain string error');
  });

  it('returns "Unknown error" for empty string input', () => {
    expect(errorMessage('')).toBe('Unknown error');
  });

  it('returns "Unknown error" for whitespace-only string input', () => {
    expect(errorMessage('   ')).toBe('Unknown error');
  });

  // Non-Error, non-string primitives coerced via String()
  it('coerces numbers via String()', () => {
    expect(errorMessage(42)).toBe('42');
  });

  it('coerces null via String()', () => {
    expect(errorMessage(null)).toBe('null');
  });

  it('coerces undefined via String()', () => {
    expect(errorMessage(undefined)).toBe('undefined');
  });

  // Plain objects with .message
  it('returns .message for plain object with string .message property', () => {
    expect(errorMessage({ message: 'structured error' })).toBe('structured error');
    expect(errorMessage({ code: 404, message: 'Not found' })).toBe('Not found');
  });

  it('returns "Unknown error" for plain object with empty string .message', () => {
    expect(errorMessage({ message: '' })).toBe('Unknown error');
  });

  it('returns "Unknown error" for plain object with whitespace-only .message', () => {
    expect(errorMessage({ message: '   ' })).toBe('Unknown error');
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

  it('returns "Unknown error" for plain object with undefined .message', () => {
    // 'in' operator returns true for properties set to undefined,
    // so 'message' in { message: undefined } is true and the non-string guard applies.
    expect(errorMessage({ message: undefined })).toBe('Unknown error');
  });

  // Objects without .message fall through to String()
  it('falls through to String() for bare object without .message', () => {
    expect(errorMessage({})).toBe('[object Object]');
  });

  it('falls through to String() for object with non-message properties', () => {
    expect(errorMessage({ code: 500 })).toBe('[object Object]');
  });

  // Hostile inputs — throwing getters and coercion methods
  it('returns "Unknown error" when value\'s toString() throws', () => {
    const obj = { toString() { throw new Error('boom'); } };
    expect(errorMessage(obj)).toBe('Unknown error');
  });

  it('returns "Unknown error" when plain object .message getter throws', () => {
    const obj = { get message() { throw new Error('boom'); } };
    expect(errorMessage(obj)).toBe('Unknown error');
  });

  it('returns "Unknown error" when Error instance .message getter throws', () => {
    const err = new Error('original');
    Object.defineProperty(err, 'message', {
      get() { throw new Error('boom'); },
      configurable: true,
    });
    expect(errorMessage(err)).toBe('Unknown error');
  });

  it('returns "Unknown error" when valueOf() and toString() both throw', () => {
    const obj = {
      valueOf() { throw new Error('boom'); },
      toString() { throw new Error('boom'); },
    };
    expect(errorMessage(obj)).toBe('Unknown error');
  });
});
