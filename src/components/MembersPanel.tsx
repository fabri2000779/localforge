/**
 * Team-plan members + pending-invitations panel for the Settings page.
 *
 * Hidden when plan != team — the cloud API 402s invite calls for
 * Hobby anyway, but we don't want to render a section that's just
 * "upgrade to use this".
 */
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { Users, UserPlus, X, MailWarning, Trash2 } from 'lucide-react';
import { useAuthStore } from '../stores/authStore';

interface Member {
  id: string;
  email: string;
  display_name: string | null;
  avatar_url: string | null;
  role: 'owner' | 'admin' | 'operator' | 'viewer';
  joined_at: number;
}

interface Invitation {
  id: string;
  email: string;
  role: 'admin' | 'operator' | 'viewer';
  expires_at: number;
  created_at: number;
}

interface OrgInfo {
  id: string;
  name: string;
  role: 'owner' | 'admin' | 'operator' | 'viewer';
  isOwner: boolean;
  createdAt: number;
  members: Member[];
}

export function MembersPanel() {
  const plan = useAuthStore((s) => s.me?.subscription.plan);
  const me = useAuthStore((s) => s.me);
  const [org, setOrg] = useState<OrgInfo | null>(null);
  const [invitations, setInvitations] = useState<Invitation[]>([]);
  const [inviteOpen, setInviteOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const o = await invoke<OrgInfo>('cloud_orgs_me');
      setOrg(o);
      try {
        const inv = await invoke<Invitation[]>('cloud_orgs_list_invitations', { orgId: o.id });
        setInvitations(inv);
      } catch {
        setInvitations([]);
      }
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (plan === 'team') void refresh();
  }, [plan, refresh]);

  if (plan !== 'team' || !org) return null;
  const canManage = ensureRoleAtLeast(org.role, 'admin');

  async function onRevoke(invId: string) {
    try {
      await invoke('cloud_orgs_revoke_invitation', { orgId: org!.id, invitationId: invId });
      setInvitations((cur) => cur.filter((i) => i.id !== invId));
    } catch (e) { setError(String(e)); }
  }

  async function onRemoveMember(userId: string) {
    if (!confirm('Remove this member?')) return;
    try {
      await invoke('cloud_orgs_remove_member', { orgId: org!.id, userId });
      void refresh();
    } catch (e) { setError(String(e)); }
  }

  return (
    <section className="card mb-5">
      <div className="section-header">
        <div className="section-title">
          <Users size={15} className="text-violet-400" />
          Team
        </div>
        {canManage && (
          <button className="btn btn-secondary" onClick={() => setInviteOpen(true)}>
            <UserPlus size={13} strokeWidth={2.2} />
            Invite member
          </button>
        )}
      </div>

      {error && <div className="auth-err mb-2">{error}</div>}

      <div className="members-list">
        {org.members.map((m) => (
          <div key={m.id} className="member-row">
            <div className="member-avatar" style={m.avatar_url ? { backgroundImage: `url(${m.avatar_url})` } : undefined}>
              {!m.avatar_url && (m.display_name ?? m.email).slice(0, 2).toUpperCase()}
            </div>
            <div className="member-meta">
              <div className="member-name">
                {m.display_name ?? m.email}
                {me?.id === m.id && <span className="member-self">you</span>}
              </div>
              <div className="member-email">{m.email}</div>
            </div>
            <span className={`role-badge role-${m.role}`}>{m.role}</span>
            {canManage && m.role !== 'owner' && m.id !== me?.id && (
              <button
                className="member-remove"
                title="Remove from org"
                onClick={() => onRemoveMember(m.id)}
              >
                <Trash2 size={13} strokeWidth={2.2} />
              </button>
            )}
          </div>
        ))}
      </div>

      {invitations.length > 0 && (
        <>
          <div className="eyebrow mt-4 mb-2">Pending invitations</div>
          <div className="members-list">
            {invitations.map((i) => (
              <div key={i.id} className="member-row pending">
                <div className="member-avatar member-avatar-pending"><MailWarning size={16} /></div>
                <div className="member-meta">
                  <div className="member-name">{i.email}</div>
                  <div className="member-email">
                    Expires {new Date(i.expires_at).toLocaleDateString()}
                  </div>
                </div>
                <span className={`role-badge role-${i.role}`}>{i.role}</span>
                {canManage && (
                  <button className="member-remove" title="Revoke invitation" onClick={() => onRevoke(i.id)}>
                    <X size={13} strokeWidth={2.2} />
                  </button>
                )}
              </div>
            ))}
          </div>
        </>
      )}

      <InviteDialog
        open={inviteOpen}
        orgId={org.id}
        onClose={() => { setInviteOpen(false); void refresh(); }}
      />
    </section>
  );
}

function ensureRoleAtLeast(role: Member['role'], min: 'owner' | 'admin'): boolean {
  const rank = (r: string) => ({ viewer: 0, operator: 1, admin: 2, owner: 3 } as Record<string, number>)[r] ?? 0;
  return rank(role) >= rank(min);
}

function InviteDialog({ open, orgId, onClose }: { open: boolean; orgId: string; onClose: () => void }) {
  const [email, setEmail] = useState('');
  const [role, setRole] = useState<'admin' | 'operator' | 'viewer'>('operator');
  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // After a successful invite we show the secret-bearing link to SHARE: the
  // emailed link works but carries no key, so only this link grants instant
  // decryption. The `#k=` secret never touched the server.
  const [link, setLink] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!open) {
      setEmail(''); setRole('operator'); setErr(null); setSubmitting(false);
      setLink(null); setCopied(false);
    }
  }, [open]);

  if (!open) return null;

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setSubmitting(true); setErr(null);
    try {
      const res = await invoke<{ id: string; secret: string }>('cloud_orgs_invite', {
        orgId, email: email.trim(), role,
      });
      setLink(`https://localforge.gg/auth/invite?token=${res.id}#k=${res.secret}`);
    } catch (e) {
      const msg = (e as { code?: string; message?: string });
      setErr(
        msg.code === 'already_member' ? `${email} is already a member.` :
        msg.code === 'plan_required'  ? 'Team plan required to invite — upgrade first.' :
                                        msg.message ?? String(e),
      );
    } finally { setSubmitting(false); }
  }

  async function copyLink() {
    if (!link) return;
    try { await navigator.clipboard.writeText(link); setCopied(true); setTimeout(() => setCopied(false), 2000); }
    catch { /* clipboard blocked — the field is selectable anyway */ }
  }

  return createPortal(
    <div className="auth-overlay" onClick={onClose} role="dialog" aria-modal="true">
      <div className="auth-modal" onClick={(e) => e.stopPropagation()} style={{ maxWidth: 460 }}>
        <button className="auth-close" onClick={onClose}><X size={16} strokeWidth={2.2} /></button>
        {link ? (
          <>
            <div className="auth-header">
              <h2>Invitation created</h2>
              <p>We emailed {email} a link. For instant access to your encrypted
                 servers, share <strong>this</strong> link directly — it carries
                 the decryption key (in the part after <code>#</code>, which never
                 reaches our servers).</p>
            </div>
            <div
              className="font-mono text-[12px] text-slate-200 bg-[var(--color-surface)] border border-[var(--color-border)] rounded-md px-3 py-2 break-all select-all"
            >
              {link}
            </div>
            <button className="auth-submit mt-3" type="button" onClick={copyLink}>
              {copied ? 'Copied!' : 'Copy invite link'}
            </button>
            <p className="text-xs text-slate-500 mt-2">
              If they use the emailed link instead, they'll still join — they just
              won't see server details until you're next online to grant them.
            </p>
          </>
        ) : (
          <>
            <div className="auth-header">
              <h2>Invite teammate</h2>
              <p>They'll get an email with a one-shot link valid for 7 days.</p>
            </div>
            <form className="auth-form" onSubmit={onSubmit}>
              <label className="auth-field">
                <span>Email</span>
                <input type="email" required autoComplete="off" placeholder="teammate@example.com"
                       value={email} onChange={(e) => setEmail(e.target.value)} />
              </label>
              <label className="auth-field">
                <span>Role</span>
                <select value={role} onChange={(e) => setRole(e.target.value as 'admin' | 'operator' | 'viewer')}>
                  <option value="viewer">Viewer — read-only dashboard + logs</option>
                  <option value="operator">Operator — start/stop/console/files</option>
                  <option value="admin">Admin — also manage members</option>
                </select>
              </label>
              {err && <div className="auth-err">{err}</div>}
              <button className="auth-submit" type="submit" disabled={submitting || !email.includes('@')}>
                {submitting ? '…' : 'Send invitation'}
              </button>
            </form>
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}
