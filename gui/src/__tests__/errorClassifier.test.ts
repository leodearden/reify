import { describe, it, expect } from 'vitest';
import { classifyError, errorMessage } from '../utils/errorClassifier';

describe('errorClassifier', () => {
  describe('auth errors', () => {
    it('classifies "auth" pattern as auth error', () => {
      const result = classifyError('Authentication failed');
      expect(result.type).toBe('auth');
      expect(result.userMessage).toBe('Authentication required. Run `claude login` in your terminal.');
    });

    it('classifies "unauthorized" pattern as auth error', () => {
      const result = classifyError('Unauthorized access');
      expect(result.type).toBe('auth');
      expect(result.userMessage).toBe('Authentication required. Run `claude login` in your terminal.');
    });

    it('classifies "401" pattern as auth error', () => {
      const result = classifyError('HTTP 401 error from API');
      expect(result.type).toBe('auth');
      expect(result.userMessage).toBe('Authentication required. Run `claude login` in your terminal.');
    });
  });

  describe('rate limit errors', () => {
    it('classifies "rate limit" pattern as rate-limit error', () => {
      const result = classifyError('Rate limit exceeded');
      expect(result.type).toBe('rate-limit');
      expect(result.userMessage).toBe('Rate limited. Please wait and try again.');
    });

    it('classifies "ratelimit" pattern as rate-limit error', () => {
      const result = classifyError('ratelimit reached');
      expect(result.type).toBe('rate-limit');
      expect(result.userMessage).toBe('Rate limited. Please wait and try again.');
    });

    it('classifies "429" pattern as rate-limit error', () => {
      const result = classifyError('HTTP 429 Too Many Requests');
      expect(result.type).toBe('rate-limit');
      expect(result.userMessage).toBe('Rate limited. Please wait and try again.');
    });
  });

  describe('network errors', () => {
    it('classifies "network" pattern as network error', () => {
      const result = classifyError('Network error occurred');
      expect(result.type).toBe('network');
      expect(result.userMessage).toBe('Connection failed. Check your network.');
    });

    it('classifies "ECONNREFUSED" pattern as network error', () => {
      const result = classifyError('connect ECONNREFUSED 127.0.0.1:3000');
      expect(result.type).toBe('network');
      expect(result.userMessage).toBe('Connection failed. Check your network.');
    });

    it('classifies "fetch" pattern as network error', () => {
      const result = classifyError('Failed to fetch');
      expect(result.type).toBe('network');
      expect(result.userMessage).toBe('Connection failed. Check your network.');
    });
  });

  describe('sidecar errors', () => {
    it('classifies "disconnect" pattern as sidecar error', () => {
      const result = classifyError('Process disconnected unexpectedly');
      expect(result.type).toBe('sidecar');
      expect(result.userMessage).toBe('Claude session disconnected. Click to restart.');
    });

    it('classifies "crash" pattern as sidecar error', () => {
      const result = classifyError('Sidecar crash detected');
      expect(result.type).toBe('sidecar');
      expect(result.userMessage).toBe('Claude session disconnected. Click to restart.');
    });

    it('classifies "exit" pattern as sidecar error', () => {
      const result = classifyError('Process exit code 1');
      expect(result.type).toBe('sidecar');
      expect(result.userMessage).toBe('Claude session disconnected. Click to restart.');
    });

    it('classifies "spawn" pattern as sidecar error', () => {
      const result = classifyError('Failed to spawn sidecar');
      expect(result.type).toBe('sidecar');
      expect(result.userMessage).toBe('Claude session disconnected. Click to restart.');
    });
  });

  describe('errorMessage re-export smoke test', () => {
    it('is exported from errorClassifier', () => {
      expect(typeof errorMessage).toBe('function');
    });

    it('delegates to @reify/shared-utils implementation', () => {
      expect(errorMessage(new Error('x'))).toBe('x');
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

  describe('unknown errors', () => {
    it('classifies unmatched errors as unknown', () => {
      const result = classifyError('Something unexpected happened');
      expect(result.type).toBe('unknown');
      expect(result.userMessage).toBe('Something unexpected happened');
    });

    it('passes through original message for unknown errors', () => {
      const msg = 'Custom error with no known pattern';
      const result = classifyError(msg);
      expect(result.type).toBe('unknown');
      expect(result.userMessage).toBe(msg);
    });
  });
});
