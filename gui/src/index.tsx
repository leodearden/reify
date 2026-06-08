import { installConsoleErrorCapture } from './debug/consoleErrors';

// Install the console-error ring buffer at the very top of the entry module so
// that startup errors are captured BEFORE applyTheme()/render() and BEFORE the
// late REIFY_DEBUG-gated initDebugBridge. Always-on — not gated on REIFY_DEBUG.
installConsoleErrorCapture();

import { render } from 'solid-js/web';
import { ErrorBoundary } from 'solid-js';
import { applyTheme } from './theme';
import App from './App';
import './global.css';

applyTheme();

const root = document.getElementById('root');
if (root) {
  render(
    () => (
      <ErrorBoundary
        fallback={(err: Error) => (
          <div
            data-testid="error-boundary-fallback"
            style={{
              display: 'flex',
              'flex-direction': 'column',
              'align-items': 'center',
              'justify-content': 'center',
              height: '100vh',
              'background-color': 'var(--reify-bg, #1e1e2e)',
              color: 'var(--reify-text, #cdd6f4)',
              'font-family': 'system-ui, sans-serif',
            }}
          >
            <h2>Something went wrong</h2>
            <p style={{ color: 'var(--reify-subtext, #a6adc8)', 'max-width': '600px', 'text-align': 'center' }}>
              {err.message}
            </p>
            <button
              onClick={() => location.reload()}
              style={{
                'margin-top': '16px',
                padding: '8px 20px',
                'background-color': 'var(--reify-accent, #89b4fa)',
                color: 'var(--reify-bg, #1e1e2e)',
                border: 'none',
                'border-radius': '6px',
                cursor: 'pointer',
                'font-size': '14px',
              }}
            >
              Reload
            </button>
          </div>
        )}
      >
        <App />
      </ErrorBoundary>
    ),
    root,
  );
}
