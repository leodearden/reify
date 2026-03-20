import { describe, it, expect } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
import App from '../App';

describe('App', () => {
  it('renders app-layout container', () => {
    render(() => <App />);
    expect(screen.getByTestId('app-layout')).toBeTruthy();
  });

  it('renders editor panel with label', () => {
    render(() => <App />);
    expect(screen.getByTestId('editor-panel')).toBeTruthy();
    expect(screen.getByText('Editor')).toBeTruthy();
  });

  it('renders viewport panel with label', () => {
    render(() => <App />);
    expect(screen.getByTestId('viewport-panel')).toBeTruthy();
    expect(screen.getByText('3D Viewport')).toBeTruthy();
  });

  it('renders side panel with Properties and Constraints sub-panels', () => {
    render(() => <App />);
    expect(screen.getByTestId('side-panel')).toBeTruthy();
    expect(screen.getByTestId('property-editor-panel')).toBeTruthy();
    expect(screen.getByTestId('constraints-panel')).toBeTruthy();
    expect(screen.getByText('Properties')).toBeTruthy();
    expect(screen.getByText('Constraints')).toBeTruthy();
  });
});
