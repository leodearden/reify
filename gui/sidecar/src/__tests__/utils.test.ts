import { describe, it, expect } from 'vitest';
import { errorMessage } from '../utils.js';
import { errorMessage as canonical } from '@reify/shared-utils';

describe('errorMessage re-export smoke test', () => {
  it('is exported from utils', () => {
    expect(typeof errorMessage).toBe('function');
  });

  it('delegates to @reify/shared-utils implementation', () => {
    expect(errorMessage(new Error('x'))).toBe('x');
  });

  it('is the exact same function reference as @reify/shared-utils', () => {
    expect(errorMessage).toBe(canonical);
  });
});
