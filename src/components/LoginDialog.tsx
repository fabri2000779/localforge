/**
 * Modal sign-in / sign-up. Triggered from the topbar's "Sign in" button
 * or from any "you need an account for this" CTA elsewhere in the app.
 *
 * Cancelable at any time — login is OPTIONAL. The user can close the
 * dialog and keep using LocalForge as a free local-only app.
 */
import { useEffect, useState, type FormEvent } from 'react';
import { X } from 'lucide-react';
import { useAuthStore } from '../stores/authStore';

type Mode = 'login' | 'signup' | 'forgot';

interface Props {
  open: boolean;
  onClose: () => void;
  /** Initial mode. Defaults to login. */
  initialMode?: Mode;
}

export function LoginDialog({ open, onClose, initialMode = 'login' }: Props) {
  const [mode, setMode] = useState<Mode>(initialMode);
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [submitted, setSubmitted] = useState(false);

  const me = useAuthStore((s) => s.me);
  const loading = useAuthStore((s) => s.loading);
  const error = useAuthStore((s) => s.error);
  const signupEmail = useAuthStore((s) => s.signupEmail);
  const loginEmail = useAuthStore((s) => s.loginEmail);
  const loginOAuth = useAuthStore((s) => s.loginOAuth);
  const requestReset = useAuthStore((s) => s.requestPasswordReset);

  // Close automatically when sign-in succeeds (via either email/pwd or
  // the OAuth deep-link callback flipping the store).
  useEffect(() => {
    if (open && me) onClose();
  }, [open, me, onClose]);

  // Reset transient state on close.
  useEffect(() => {
    if (!open) {
      setSubmitted(false);
      setMode(initialMode);
    }
  }, [open, initialMode]);

  if (!open) return null;

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setSubmitted(true);
    if (mode === 'signup') {
      await signupEmail(email.trim(), password, displayName.trim() || undefined);
    } else if (mode === 'login') {
      await loginEmail(email.trim(), password);
    } else if (mode === 'forgot') {
      await requestReset(email.trim());
    }
  }

  const subtitle = (() => {
    if (mode === 'signup') return 'Optional — only needed if you want cloud sync, multi-device, or to invite teammates.';
    if (mode === 'forgot') return "We'll email you a link to set a new password.";
    return 'Sign in to your LocalForge cloud account.';
  })();

  return (
    <div className="auth-overlay" onClick={onClose} aria-modal="true" role="dialog">
      <div className="auth-modal" onClick={(e) => e.stopPropagation()}>
        <button className="auth-close" onClick={onClose} aria-label="Close">
          <X size={16} strokeWidth={2.2} />
        </button>

        <div className="auth-header">
          <h2>{mode === 'signup' ? 'Create your account' : mode === 'forgot' ? 'Reset password' : 'Welcome back'}</h2>
          <p>{subtitle}</p>
        </div>

        {mode !== 'forgot' && (
          <>
            <div className="auth-oauth-grid">
              <OAuthBtn provider="apple"   disabled={loading} onClick={() => loginOAuth('apple')} />
              <OAuthBtn provider="discord" disabled={loading} onClick={() => loginOAuth('discord')} />
              <OAuthBtn provider="google"  disabled={loading} onClick={() => loginOAuth('google')} />
              <OAuthBtn provider="github"  disabled={loading} onClick={() => loginOAuth('github')} />
            </div>
            <div className="auth-divider"><span>or with email</span></div>
          </>
        )}

        <form className="auth-form" onSubmit={onSubmit}>
          {mode === 'signup' && (
            <Field label="Display name (optional)" value={displayName} onChange={setDisplayName} autoComplete="name" placeholder="Fabrizio" />
          )}
          <Field label="Email" type="email" value={email} onChange={setEmail} autoComplete="email" required placeholder="you@example.com" />
          {mode !== 'forgot' && (
            <Field
              label="Password"
              type="password"
              value={password}
              onChange={setPassword}
              autoComplete={mode === 'signup' ? 'new-password' : 'current-password'}
              required
              minLength={10}
              placeholder={mode === 'signup' ? 'At least 10 characters' : undefined}
            />
          )}

          {error && submitted && <div className="auth-err">{prettyError(error.code, error.message ?? null, mode)}</div>}
          {mode === 'forgot' && submitted && !error && (
            <div className="auth-info">If <strong>{email}</strong> is registered, the reset link is on its way.</div>
          )}

          <button type="submit" className="auth-submit" disabled={loading}>
            {loading ? '…' :
              mode === 'signup' ? 'Create account' :
              mode === 'login'  ? 'Sign in' :
                                  'Send reset link'}
          </button>
        </form>

        <div className="auth-meta">
          {mode === 'login' && (
            <>
              <button className="auth-link" onClick={() => setMode('forgot')}>Forgot password?</button>
              <span className="auth-meta-sep">·</span>
              <button className="auth-link" onClick={() => setMode('signup')}>Create an account</button>
            </>
          )}
          {mode === 'signup' && (
            <button className="auth-link" onClick={() => setMode('login')}>I already have an account</button>
          )}
          {mode === 'forgot' && (
            <button className="auth-link" onClick={() => setMode('login')}>Back to sign in</button>
          )}
        </div>
      </div>
    </div>
  );
}

interface FieldProps {
  label: string;
  value: string;
  onChange: (v: string) => void;
  type?: string;
  required?: boolean;
  minLength?: number;
  autoComplete?: string;
  placeholder?: string;
}
function Field({ label, value, onChange, type = 'text', required, minLength, autoComplete, placeholder }: FieldProps) {
  return (
    <label className="auth-field">
      <span>{label}</span>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        required={required}
        minLength={minLength}
        autoComplete={autoComplete}
        placeholder={placeholder}
        spellCheck={false}
      />
    </label>
  );
}

function OAuthBtn({ provider, disabled, onClick }: {
  provider: 'apple' | 'discord' | 'google' | 'github';
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button type="button" className={`auth-oauth oauth-${provider}`} disabled={disabled} onClick={onClick}>
      <ProviderIcon provider={provider} />
      <span>{provider === 'apple' ? 'Apple' : provider === 'discord' ? 'Discord' : provider === 'google' ? 'Google' : 'GitHub'}</span>
    </button>
  );
}

function ProviderIcon({ provider }: { provider: 'apple' | 'discord' | 'google' | 'github' }) {
  if (provider === 'apple') {
    return (
      <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
        <path d="M16.365 1.43c0 1.14-.42 2.2-1.13 3.02-.83.97-2.2 1.72-3.34 1.63-.14-1.13.46-2.32 1.12-3.06.78-.9 2.16-1.55 3.35-1.59zM20.78 17.5c-.6 1.37-.88 1.98-1.66 3.19-1.08 1.69-2.6 3.79-4.48 3.8-1.67.02-2.1-1.09-4.37-1.08-2.27.01-2.74 1.1-4.41 1.08-1.88-.01-3.32-1.91-4.4-3.6C-1.04 16.55-1.4 9.96 1.87 6.96c1.17-1.08 2.71-1.71 4.18-1.71 1.5 0 2.44 1.08 4.37 1.08 1.87 0 3-1.08 4.74-1.08 1.31 0 2.7.71 3.69 1.94-3.24 1.78-2.71 6.41.93 8.31z"/>
      </svg>
    );
  }
  if (provider === 'discord') {
    return (
      <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
        <path d="M20.317 4.37a19.79 19.79 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.74 19.74 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 0 0 .031.057 19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028 14.09 14.09 0 0 0 1.226-1.994.076.076 0 0 0-.041-.106 13.1 13.1 0 0 1-1.872-.892.077.077 0 0 1-.008-.128 10.2 10.2 0 0 0 .372-.292.074.074 0 0 1 .077-.01c3.927 1.793 8.18 1.793 12.061 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127 12.3 12.3 0 0 1-1.873.892.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028 19.84 19.84 0 0 0 6.002-3.03.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03zM8.02 15.33c-1.182 0-2.157-1.085-2.157-2.419 0-1.333.956-2.419 2.157-2.419 1.21 0 2.176 1.096 2.157 2.42 0 1.333-.956 2.418-2.157 2.418zm7.975 0c-1.183 0-2.157-1.085-2.157-2.419 0-1.333.955-2.419 2.157-2.419 1.21 0 2.176 1.096 2.157 2.42 0 1.333-.946 2.418-2.157 2.418z"/>
      </svg>
    );
  }
  if (provider === 'google') {
    return (
      <svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true">
        <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 0 1-2.2 3.32v2.76h3.56c2.08-1.92 3.28-4.74 3.28-8.09z"/>
        <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.56-2.76c-.99.66-2.26 1.06-3.72 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84A11 11 0 0 0 12 23z"/>
        <path fill="#FBBC05" d="M5.84 14.11A6.6 6.6 0 0 1 5.5 12c0-.73.13-1.44.34-2.11V7.05H2.18A11 11 0 0 0 1 12c0 1.78.43 3.47 1.18 4.95l3.66-2.84z"/>
        <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.65l3.15-3.15C17.45 2.11 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.05l3.66 2.84C6.71 7.31 9.14 5.38 12 5.38z"/>
      </svg>
    );
  }
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M12 0C5.37 0 0 5.37 0 12c0 5.3 3.44 9.8 8.21 11.39.6.11.82-.26.82-.58v-2.16c-3.34.73-4.04-1.42-4.04-1.42-.55-1.39-1.34-1.76-1.34-1.76-1.1-.75.08-.73.08-.73 1.21.09 1.85 1.24 1.85 1.24 1.08 1.84 2.83 1.31 3.52 1 .11-.78.42-1.31.76-1.61-2.66-.3-5.47-1.33-5.47-5.92 0-1.31.47-2.38 1.24-3.22-.13-.31-.54-1.52.11-3.17 0 0 1.01-.32 3.31 1.23A11.5 11.5 0 0 1 12 5.8c1.02 0 2.05.14 3.01.41 2.3-1.55 3.31-1.23 3.31-1.23.65 1.65.24 2.86.12 3.17.77.84 1.24 1.91 1.24 3.22 0 4.6-2.81 5.61-5.48 5.91.43.37.82 1.1.82 2.22v3.29c0 .32.22.7.83.58A12 12 0 0 0 24 12c0-6.63-5.37-12-12-12z"/>
    </svg>
  );
}

function prettyError(code: string, msg: string | null, mode: Mode): string {
  switch (code) {
    case 'email_in_use': return 'That email is already registered. Try signing in instead.';
    case 'invalid_credentials': return 'Wrong email or password.';
    case 'rate_limited': return 'Too many attempts — wait a few minutes.';
    case 'network': return 'No connection to the cloud. Check your internet.';
  }
  // signup-side validation hit by Zod looks like "..."
  if (mode === 'signup' && code.startsWith('http_400')) return 'Check the form — email needs to be valid and password ≥10 chars.';
  return msg ?? 'Something went wrong. Try again.';
}
