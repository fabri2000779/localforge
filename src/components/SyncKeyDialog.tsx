/** Envelope-encryption setup modal driven by `syncKeyStatus` (create or enter the sync password).
 *  Dismissable: local-only use is first-class; OAuth users see it on first sign-in. */
import { useEffect, useState, type FormEvent } from 'react';
import { X, ShieldCheck, KeyRound, AlertTriangle } from 'lucide-react';
import { useAuthStore } from '../stores/authStore';

export function SyncKeyDialog() {
  const me = useAuthStore((s) => s.me);
  const status = useAuthStore((s) => s.syncKeyStatus);
  const openTick = useAuthStore((s) => s.openSyncKeyTick);
  const setupSyncKey = useAuthStore((s) => s.setupSyncKey);
  const unlockSyncKey = useAuthStore((s) => s.unlockSyncKey);

  // Explicit dismissal for this session; openSyncKeyTick re-opens it.
  const [dismissed, setDismissed] = useState(false);
  const [passphrase, setPassphrase] = useState('');
  const [confirm, setConfirm] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  // Reset transient state on sign-in/out, status flips and explicit opens.
  useEffect(() => {
    setPassphrase('');
    setConfirm('');
    setErr(null);
    setSubmitting(false);
    setDismissed(false);
  }, [me?.id, status, openTick]);

  const shouldShow =
    !!me &&
    status !== null &&
    status !== 'unlocked' &&
    !dismissed;

  if (!shouldShow) return null;

  const isSetup = status === 'not_set_up';

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setErr(null);
    if (passphrase.length < 12) {
      setErr('Use at least 12 characters. The passphrase protects every config you sync.');
      return;
    }
    if (isSetup && passphrase !== confirm) {
      setErr("Passphrases don't match.");
      return;
    }
    setSubmitting(true);
    const ok = isSetup
      ? await setupSyncKey(passphrase)
      : await unlockSyncKey(passphrase);
    setSubmitting(false);
    if (ok) {
      // The status flip inside setup/unlock hides the dialog on the next render.
      return;
    }
    setErr(isSetup
      ? 'Could not save the sync key. Check your connection and try again.'
      : "That doesn't match the passphrase you used on your first device. Try again, or set up a new one from Settings → Show recovery key.");
  }

  return (
    <div className="auth-overlay" role="dialog" aria-modal="true">
      <div className="auth-modal" style={{ maxWidth: 480 }}>
        <button className="auth-close" onClick={() => setDismissed(true)} aria-label="Close">
          <X size={16} strokeWidth={2.2} />
        </button>

        <div className="auth-header">
          <h2>
            {isSetup ? 'Create your sync password' : 'Unlock your synced data'}
          </h2>
          <p>
            {isSetup
              ? "We use this to encrypt every config we sync to the cloud. We can't read it, and you'll need it to sign in on another device — pick something you'll remember or save it to a password manager."
              : "Enter the sync password you set up on your first device. We use it to decrypt your synced configs locally — the cloud never sees it."}
          </p>
        </div>

        <form className="auth-form" onSubmit={onSubmit}>
          <label className="auth-field">
            <span>{isSetup ? 'Sync password' : 'Sync password'}</span>
            <input
              type="password"
              autoComplete={isSetup ? 'new-password' : 'current-password'}
              required
              minLength={12}
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
              placeholder="At least 12 characters"
              autoFocus
              spellCheck={false}
            />
          </label>
          {isSetup && (
            <label className="auth-field">
              <span>Confirm</span>
              <input
                type="password"
                autoComplete="new-password"
                required
                minLength={12}
                value={confirm}
                onChange={(e) => setConfirm(e.target.value)}
                spellCheck={false}
              />
            </label>
          )}

          {err && <div className="auth-err">{err}</div>}

          {isSetup && (
            <div className="vault-warn">
              <ShieldCheck size={14} className="shrink-0 text-emerald-400 mt-[2px]" />
              <span>
                This passphrase never leaves your device. If you forget it,
                cloud-synced configs will be unrecoverable (your local
                servers are unaffected).
              </span>
            </div>
          )}
          {!isSetup && (
            <div className="vault-warn vault-warn-yellow">
              <AlertTriangle size={14} className="shrink-0 text-amber-400 mt-[2px]" />
              <span>
                Forgot it? You can also restore from the recovery key shown
                on your first device — Settings → "Show recovery key".
              </span>
            </div>
          )}

          <button type="submit" className="auth-submit" disabled={submitting}>
            <KeyRound size={13} strokeWidth={2.2} />
            {submitting ? '…' : isSetup ? 'Create sync key' : 'Unlock'}
          </button>
        </form>

        <div className="auth-meta">
          <button type="button" className="link-button" onClick={() => setDismissed(true)}>
            Skip for now
          </button>
        </div>
      </div>
    </div>
  );
}
