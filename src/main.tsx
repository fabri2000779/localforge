import ReactDOM from 'react-dom/client';
import App from './App';

// Note: StrictMode disabled because it causes effects to run twice,
// which interferes with the log streaming attach/detach lifecycle
ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <App />
);
