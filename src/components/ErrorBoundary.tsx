/**
 * Last-resort safety net. React 19 in production silently renders
 * nothing when a component throws — which is how the
 * "useNavigate-outside-Router" bug in App.tsx looked like a blank
 * window. With this wrapper around the tree, a thrown component
 * lands here and the user at least sees the error + a "reload" button
 * instead of an opaque black screen.
 *
 * Errors are logged to console for devs running with stderr captured.
 * Not auto-reported anywhere yet — would be the next iteration.
 */
import { Component, type ErrorInfo, type ReactNode } from 'react';
import { AlertOctagon } from 'lucide-react';

interface Props { children: ReactNode }
interface State { error: Error | null }

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error('[ErrorBoundary]', error, info.componentStack);
  }

  render(): ReactNode {
    if (!this.state.error) return this.props.children;
    return (
      <div style={{
        minHeight: '100vh',
        background: '#07090f',
        color: '#e2e8f0',
        fontFamily: 'Inter, system-ui, sans-serif',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 32,
      }}>
        <div style={{
          maxWidth: 540,
          background: 'rgba(14,19,28,.8)',
          border: '1px solid rgba(248,113,113,.3)',
          borderRadius: 14,
          padding: 28,
        }}>
          <AlertOctagon size={28} color="#fca5a5" style={{ marginBottom: 12 }} />
          <h1 style={{ fontSize: 18, fontWeight: 600, color: '#fff', margin: '0 0 8px' }}>
            LocalForge crashed
          </h1>
          <p style={{ fontSize: 13.5, color: '#94a3b8', margin: '0 0 14px', lineHeight: 1.5 }}>
            Something in the UI threw an exception we didn't expect. Your
            local data is safe — Docker containers and configs aren't
            touched by the UI process. Try reloading; if it keeps
            happening, please share the error below.
          </p>
          <pre style={{
            background: '#0b1020',
            border: '1px solid rgba(148,163,184,.16)',
            borderRadius: 8,
            padding: 12,
            fontSize: 11.5,
            color: '#fca5a5',
            overflow: 'auto',
            maxHeight: 200,
            margin: '0 0 14px',
          }}>{this.state.error.stack ?? this.state.error.message}</pre>
          <button
            onClick={() => window.location.reload()}
            style={{
              padding: '8px 16px',
              borderRadius: 8,
              border: 0,
              background: 'linear-gradient(180deg,#3b82f6,#2563eb)',
              color: '#fff',
              fontWeight: 600,
              fontSize: 13,
              cursor: 'pointer',
            }}
          >
            Reload
          </button>
        </div>
      </div>
    );
  }
}
