// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import {
  loadChatPanelHeight,
  saveChatPanelHeight,
  loadChatPanelOpen,
  saveChatPanelOpen,
  CHAT_HEIGHT_KEY,
  CHAT_OPEN_KEY,
} from '../hooks/useChatPanelPersistence';

describe('useChatPanelPersistence', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  describe('loadChatPanelHeight', () => {
    it('returns null when localStorage is empty', () => {
      expect(loadChatPanelHeight()).toBeNull();
    });

    it('returns number when valid JSON number is stored', () => {
      localStorage.setItem(CHAT_HEIGHT_KEY, JSON.stringify(300));
      expect(loadChatPanelHeight()).toBe(300);
    });

    it('returns null for non-number JSON', () => {
      localStorage.setItem(CHAT_HEIGHT_KEY, JSON.stringify('not a number'));
      expect(loadChatPanelHeight()).toBeNull();
    });

    it('returns null for corrupted JSON', () => {
      localStorage.setItem(CHAT_HEIGHT_KEY, '{broken json!!!');
      expect(loadChatPanelHeight()).toBeNull();
    });
  });

  describe('saveChatPanelHeight', () => {
    it('stores number to localStorage', () => {
      saveChatPanelHeight(350);
      const stored = localStorage.getItem(CHAT_HEIGHT_KEY);
      expect(stored).not.toBeNull();
      expect(JSON.parse(stored!)).toBe(350);
    });
  });

  describe('loadChatPanelOpen', () => {
    it('returns null when localStorage is empty', () => {
      expect(loadChatPanelOpen()).toBeNull();
    });

    it('returns boolean when valid boolean is stored', () => {
      localStorage.setItem(CHAT_OPEN_KEY, JSON.stringify(true));
      expect(loadChatPanelOpen()).toBe(true);
    });

    it('returns null for non-boolean JSON', () => {
      localStorage.setItem(CHAT_OPEN_KEY, JSON.stringify('yes'));
      expect(loadChatPanelOpen()).toBeNull();
    });
  });

  describe('saveChatPanelOpen', () => {
    it('stores boolean to localStorage', () => {
      saveChatPanelOpen(true);
      const stored = localStorage.getItem(CHAT_OPEN_KEY);
      expect(stored).not.toBeNull();
      expect(JSON.parse(stored!)).toBe(true);
    });
  });
});
