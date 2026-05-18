// Dev-only Tauri shim — MUST be imported before any module that pulls in
// `@tauri-apps/api/core`, because those modules read
// `window.__TAURI_INTERNALS__` from the very first invoke call. Side-
// effect import: the shim self-skips if it detects the real Tauri runtime.
import './dev/tauri-shim';

import ReactDOM from 'react-dom/client';
import App from './App';

// Note: StrictMode disabled because it causes effects to run twice,
// which interferes with the log streaming attach/detach lifecycle
ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <App />
);
