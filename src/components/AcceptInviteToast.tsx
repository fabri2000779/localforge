/** Toast for `cloud://invite-received` (invitation email deep link); routes through login first if needed. */
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Mail, X, Check } from 'lucide-react';
import { useAuthStore } from '../stores/authStore';
import { LoginDialog } from './LoginDialog';
import { describeError } from '../utils/errors';

export function AcceptInviteToast() {
  const me = useAuthStore((s) => s.me);
  const refreshMe = useAuthStore((s) => s.refreshMe);
  const [token, setToken] = useState<string | null>(null);
  // Optional handoff secret from the invite link #fragment.
  const [secret, setSecret] = useState<string | null>(null);
  const [pendingLogin, setPendingLogin] = useState(false);
  const [accepting, setAccepting] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    listen<{ token: string; secret?: string | null }>('cloud://invite-received', (event) => {
      setToken(event.payload.token);
      setSecret(event.payload.secret ?? null);
      setErr(null);
    }).then((fn) => { unlisten = fn; });
    return () => { if (unlisten) unlisten(); };
  }, []);

  // Accept automatically once the user signs in.
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
      const orgId = await invoke<string>('cloud_orgs_accept_invite', { token, secret });
      setToken(null);
      setSecret(null);
      await refreshMe();
      // Switch to the joined org; membership can lag, so poll fetchOrgs until it appears.
      const auth = useAuthStore.getState();
      for (let i = 0; i < 5; i++) {
        await auth.fetchOrgs();
        if (useAuthStore.getState().orgs.some((o) => o.id === orgId)) break;
        await new Promise((r) => setTimeout(r, 500));
      }
      auth.setCurrentOrg(orgId);
    } catch (e) {
      const msg = e as { code?: string; message?: string };
      setErr(
        msg.code === 'wrong_account' ? "This invite is for a different email — sign out and use the right account." :
        msg.code === 'expired'       ? 'Invitation expired. Ask the inviter to resend.' :
        msg.code === 'already_accepted' ? 'You already accepted this invitation.' :
        msg.message ?? describeError(e),
      );
    } finally { setAccepting(false); }
  }

  if (!token) return null;

  // If not logged in yet, surface the login dialog with intent.
  if (!me) {
    return (
      <>
        <div className="invite-toast">
          <Mail size={16} className="shrink-0 text-sky-300" />
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
      <Mail size={16} className="shrink-0 text-sky-300" />
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
