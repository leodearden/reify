import { describe, it, expect } from 'vitest';
import { errorMessage } from '../utils.js';

describe('errorMessage', () => {
  it('returns .message for Error instances', () => {
    expect(errorMessage(new Error('something broke'))).toBe('something broke');
  });

  it('returns .message for Error subclass instances', () => {
    expect(errorMessage(new TypeError('bad type'))).toBe('bad type');
    expect(errorMessage(new RangeError('out of range'))).toBe('out of range');
  });

  it('returns the string itself for string inputs', () => {
    expect(errorMessage('plain string error')).toBe('plain string error');
  });

  it('coerces non-Error, non-string values via String()', () => {
    expect(errorMessage(42)).toBe('42');
    expect(errorMessage(null)).toBe('null');
    expect(errorMessage(undefined)).toBe('undefined');
    expect(errorMessage({ key: 'val' })).toBe('[object Object]');
  });
});
