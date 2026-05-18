/**
 * Listens for `cloud://invite-received` (the deep-link handler emits
 * this when the user clicks an invitation email). Pops a toast with
 * an Accept button. If they're not signed in, the toast routes them
 * to the LoginDialog first; we keep the token in component state and
 * accept once the user is logged in.
 */
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Mail, X, Check } from 'lucide-react';
import { useAuthStore } from '../stores/authStore';
import { LoginDialog } from './LoginDialog';

export function AcceptInviteToast() {
  const me = useAuthStore((s) => s.me);
  const refreshMe = useAuthStore((s) => s.refreshMe);
  const [token, setToken] = useState<string | null>(null);
  const [pendingLogin, setPendingLogin] = useState(false);
  const [accepting, setAccepting] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    listen<{ token: string }>('cloud://invite-received', (event) => {
      setToken(event.payload.token);
      setErr(null);
    }).then((fn) => { unlisten = fn; });
    return () => { if (unlisten) unlisten(); };
  }, []);

  // Once the user signs in (if they had a pending invite), kick the
  // accept flow automatically.
  useEffect(() => {
    if (pendingLogin && me && token) {
      setPendingLogin(false);
      void accept();
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [me, pendingLogin, token]);

  async function accept() {
    if (!token) return;
    setAccepting(true);
    try {
      await invoke<string>('cloud_orgs_accept_invite', { token });
      setToken(null);
      void refreshMe();
    } catch (e) {
      const msg = e as { code?: string; message?: string };
      setErr(
        msg.code === 'wrong_account' ? "This invite is for a different email — sign out and use the right account." :
        msg.code === 'expired'       ? 'Invitation expired. Ask the inviter to resend.' :
        msg.code === 'already_accepted' ? 'You already accepted this invitation.' :
        msg.message ?? String(e),
      );
    } finally { setAccepting(false); }
  }

  if (!token) return null;

  // If not logged in yet, surface the login dialog with intent.
  if (!me) {
    return (
      <>
        <div className="invite-toast">
          <Mail size={16} className="shrink-0 text-violet-300" />
          <div className="flex-1">
            <strong>Team invitation</strong>
            <p>Sign in with the same email you were invited under to accept.</p>
          </div>
          <button className="btn-primary" onClick={() => setPendingLogin(true)}>Sign in</button>
          <button className="invite-toast-close" onClick={() => setToken(null)}>
            <X size={14} strokeWidth={2.2} />
          </button>
        </div>
        <LoginDialog open={pendingLogin} onClose={() => setPendingLogin(false)} />
      </>
    );
  }

  return (
    <div className="invite-toast">
      <Mail size={16} className="shrink-0 text-violet-300" />
      <div className="flex-1">
        <strong>You've been invited to a workspace</strong>
        <p>{err ?? "Click 'Accept' to join."}</p>
      </div>
      <button className="btn-primary" onClick={accept} disabled={accepting}>
        <Check size={13} strokeWidth={2.2} />
        {accepting ? '…' : 'Accept'}
      </button>
      <button className="invite-toast-close" onClick={() => setToken(null)}>
        <X size={14} strokeWidth={2.2} />
      </button>
    </div>
  );
}
