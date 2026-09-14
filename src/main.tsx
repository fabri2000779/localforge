// Dev-only Tauri shim; must be imported before anything that touches @tauri-apps/api/core.
import './dev/tauri-shim';

// Self-hosted brand fonts so they render offline.
import '@fontsource/space-grotesk/400.css';
import '@fontsource/space-grotesk/500.css';
import '@fontsource/space-grotesk/600.css';
import '@fontsource/space-grotesk/700.css';
import '@fontsource/jetbrains-mono/400.css';
import '@fontsource/jetbrains-mono/500.css';
import '@fontsource/jetbrains-mono/600.css';

import ReactDOM from 'react-dom/client';
import App from './App';
import { ErrorBoundary } from './components/ErrorBoundary';

// No StrictMode: double-invoked effects break the log-stream attach/detach lifecycle.
// ErrorBoundary is outermost so any render throw shows an error UI instead of a blank window.
ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <ErrorBoundary>
    <App />
  </ErrorBoundary>
);
