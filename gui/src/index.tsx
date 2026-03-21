import { render } from 'solid-js/web';
import { applyTheme } from './theme';
import App from './App';
import './global.css';

applyTheme();

const root = document.getElementById('root');
if (root) {
  render(() => <App />, root);
}
