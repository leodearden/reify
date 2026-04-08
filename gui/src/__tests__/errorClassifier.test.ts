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

    it('returns "Unknown error" for Error with empty message', () => {
      expect(errorMessage(new Error(''))).toBe('Unknown error');
    });

    it('returns "Unknown error" for empty string input', () => {
      expect(errorMessage('')).toBe('Unknown error');
    });

    it('returns "Unknown error" for Error with whitespace-only message', () => {
      expect(errorMessage(new Error('   '))).toBe('Unknown error');
    });

    it('returns "Unknown error" for whitespace-only string input', () => {
      expect(errorMessage('   ')).toBe('Unknown error');
    });

    it('returns "Unknown error" for plain object with whitespace-only .message', () => {
      expect(errorMessage({ message: '   ' })).toBe('Unknown error');
    });

    it('coerces non-Error, non-string values via String()', () => {
      expect(errorMessage(42)).toBe('42');
      expect(errorMessage(null)).toBe('null');
      expect(errorMessage(undefined)).toBe('undefined');
      expect(errorMessage({ key: 'val' })).toBe('[object Object]');
    });

    it('returns .message for plain object with string .message property', () => {
      expect(errorMessage({ code: 404, message: 'Not found' })).toBe('Not found');
      expect(errorMessage({ message: 'structured error' })).toBe('structured error');
    });

    it('returns "Unknown error" for plain object with empty string .message', () => {
      expect(errorMessage({ message: '' })).toBe('Unknown error');
    });

    it('returns Unknown error for plain object with non-string .message', () => {
      expect(errorMessage({ message: 42 })).toBe('Unknown error');
      expect(errorMessage({ message: null })).toBe('Unknown error');
    });

    it('falls through to String() for plain object without .message', () => {
      expect(errorMessage({ code: 500 })).toBe('[object Object]');
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
