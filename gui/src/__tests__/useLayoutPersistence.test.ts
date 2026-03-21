// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import { loadPanelLayout, savePanelLayout, STORAGE_KEY } from '../hooks/useLayoutPersistence';

describe('useLayoutPersistence', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('loadPanelLayout returns null when no data in localStorage', () => {
    const result = loadPanelLayout();
    expect(result).toBeNull();
  });

  it('loadPanelLayout returns parsed PanelLayout when valid JSON exists', () => {
    const layout = { editorWidth: 400, sideWidth: 350, propertyHeight: 250 };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(layout));

    const result = loadPanelLayout();
    expect(result).toEqual(layout);
  });

  it('savePanelLayout writes serialized layout to localStorage', () => {
    const layout = { editorWidth: 400, sideWidth: 350, propertyHeight: 250 };
    savePanelLayout(layout);

    const stored = localStorage.getItem(STORAGE_KEY);
    expect(stored).not.toBeNull();
    expect(JSON.parse(stored!)).toEqual(layout);
  });

  it('loadPanelLayout returns null when localStorage contains corrupted JSON', () => {
    localStorage.setItem(STORAGE_KEY, '{not valid json!!!');

    const result = loadPanelLayout();
    expect(result).toBeNull();
  });

  it('loadPanelLayout returns null when stored object is missing required fields', () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ editorWidth: 400 }));

    const result = loadPanelLayout();
    expect(result).toBeNull();
  });
});
