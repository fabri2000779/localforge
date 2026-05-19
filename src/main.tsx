// Dev-only Tauri shim — MUST be imported before any module that pulls in
// `@tauri-apps/api/core`, because those modules read
// `window.__TAURI_INTERNALS__` from the very first invoke call. Side-
// effect import: the shim self-skips if it detects the real Tauri runtime.
import './dev/tauri-shim';

import ReactDOM from 'react-dom/client';
import App from './App';
import { ErrorBoundary } from './components/ErrorBoundary';

// Note: StrictMode disabled because it causes effects to run twice,
// which interferes with the log streaming attach/detach lifecycle
//
// ErrorBoundary is the outermost wrapper so that ANYTHING throwing —
// including App itself or a top-level provider — surfaces as a visible
// error UI instead of a black screen. React 19 production builds
// otherwise just render nothing on unhandled render exceptions, which
// is how the v0.1.9–v0.1.16 "blank window when Docker is stopped" bug
// looked: the useNavigate-outside-Router throw was silently swallowed.
ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <ErrorBoundary>
    <App />
  </ErrorBoundary>
);
