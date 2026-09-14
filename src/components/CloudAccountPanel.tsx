/** Account / billing panel (Settings): signed out, free (upgrade CTAs) or paid (manage billing).
 *  Checkout and the portal open in the system browser; cloud_me picks up the result. */
import { useEffect, useState } from 'react';
import {
  Cloud, ExternalLink, Mail, ShieldCheck, AlertTriangle, LogOut,
  RefreshCw, KeyRound, Download, Upload,
} from 'lucide-react';
import { useAuthStore } from '../stores/authStore';
import { LoginDialog } from './LoginDialog';
import { RecoveryKeyDialog } from './RecoveryKeyDialog';

export function CloudAccountPanel() {
  const me = useAuthStore((s) => s.me);
  const loading = useAuthStore((s) => s.loading);
  const refreshMe = useAuthStore((s) => s.refreshMe);
  const openCheckout = useAuthStore((s) => s.openCheckout);
  const openPortal = useAuthStore((s) => s.openPortal);
  const resendVerification = useAuthStore((s) => s.resendVerification);
  const logout = useAuthStore((s) => s.logout);

  const syncing = useAuthStore((s) => s.syncing);
  const lastSyncedAt = useAuthStore((s) => s.lastSyncedAt);
  const lastSyncResult = useAuthStore((s) => s.lastSyncResult);
  const syncNow = useAuthStore((s) => s.syncNow);
  const exportData = useAuthStore((s) => s.exportData);
  const syncKeyStatus = useAuthStore((s) => s.syncKeyStatus);
  const refreshSyncKeyStatus = useAuthStore((s) => s.refreshSyncKeyStatus);
  const openSyncKeyDialog = useAuthStore((s) => s.openSyncKeyDialog);

  const [loginOpen, setLoginOpen] = useState(false);
  const [resendOk, setResendOk] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [keyDialog, setKeyDialog] = useState<'show' | 'import' | null>(null);
  // Mount-time clock for the day-granular retention countdown (keeps Date.now() out of render).
  const [now] = useState(() => Date.now());

  // Re-pull /me on mount so the plan is fresh after an upgrade in the browser.
  useEffect(() => {
    void refreshMe();
    void refreshSyncKeyStatus();
  }, [refreshMe, refreshSyncKeyStatus]);

  // Signed-out card
  if (me === null || me === undefined) {
    return (
      <section className="card mb-5">
        <div className="section-header">
          <div className="section-title">
            <Cloud size={15} className="text-sky-400" />
            Cloud account
          </div>
        </div>
        <div className="cloud-empty">
          <p>Sign in to sync your servers across devices, get crash alerts and (with Team) invite teammates.</p>
          <p className="cloud-empty-sub">Optional — LocalForge works fully without an account.</p>
          <button className="btn btn-primary mt-3" disabled={loading} onClick={() => setLoginOpen(true)}>
            Sign in / Create account
          </button>
        </div>
        <LoginDialog open={loginOpen} onClose={() => setLoginOpen(false)} />
      </section>
    );
  }

  const plan = me.subscription.plan;
  const periodEnd = me.subscription.currentPeriodEnd;
  const cancelling = me.subscription.cancelAtPeriodEnd;
  const purgeAt = me.subscription.purgeAt;
  const planLabel = plan[0]!.toUpperCase() + plan.slice(1);

  // Retention banner thresholds match the cloud cron's emails (T-7 / T-1).
  const purgeDays = purgeAt ? Math.max(0, Math.ceil((purgeAt - now) / 86_400_000)) : null;
  const showPurgeBanner = purgeAt && purgeDays !== null && purgeDays <= 14;

  async function withBusy(key: string, fn: () => Promise<unknown>): Promise<void> {
    setBusy(key);
    await fn();
    setBusy(null);
  }

  return (
    <>
      <section className="card mb-5">
        <div className="section-header">
          <div className="section-title">
            <Cloud size={15} className="text-sky-400" />
            Cloud account
          </div>
          <button className="btn btn-ghost" onClick={() => void logout()} title="Sign out">
            <LogOut size={13} strokeWidth={2.2} />
            <span>Sign out</span>
          </button>
        </div>

        <div className="cloud-identity">
          <div>
            <div className="cloud-name">{me.displayName ?? me.email}</div>
            <div className="cloud-email">{me.email}</div>
          </div>
          <span className={`plan-badge plan-${plan}`}>{planLabel}</span>
        </div>

        {!me.emailVerifiedAt && (
          <div className="cloud-warn">
            <AlertTriangle size={14} className="shrink-0 text-amber-400 mt-[2px]" />
            <div className="flex-1">
              <strong>Confirm your email</strong>
              <p>We sent a link to <code>{me.email}</code>. Click it to unlock all features.</p>
            </div>
            <button
              className="btn btn-secondary"
              disabled={resendOk || busy === 'resend'}
              onClick={async () => {
                await withBusy('resend', async () => {
                  const ok = await resendVerification();
                  if (ok) setResendOk(true);
                });
              }}
            >
              <Mail size={13} strokeWidth={2.2} />
              {resendOk ? 'Sent' : busy === 'resend' ? '…' : 'Resend'}
            </button>
          </div>
        )}

        {showPurgeBanner && (
          <div className="cloud-warn cloud-warn-red">
            <AlertTriangle size={14} className="shrink-0 text-red-400 mt-[2px]" />
            <div className="flex-1">
              <strong>Your cloud data will be deleted {purgeDays === 0 ? 'today' : purgeDays === 1 ? 'tomorrow' : `in ${purgeDays} days`}</strong>
              <p>Re-subscribe to keep your synced configs, or export them below before the deletion runs.</p>
            </div>
            <button className="btn btn-primary" onClick={() => withBusy('hobby', () => openCheckout('hobby'))} disabled={busy === 'hobby'}>
              Re-subscribe
            </button>
          </div>
        )}

        {syncKeyStatus && syncKeyStatus !== 'unlocked' && (
          <div className="cloud-warn">
            <KeyRound size={14} className="shrink-0 text-amber-400 mt-[2px]" />
            <div className="flex-1">
              <strong>
                {syncKeyStatus === 'not_set_up'
                  ? 'Set up your sync password'
                  : 'Unlock your synced data'}
              </strong>
              <p>
                {syncKeyStatus === 'not_set_up'
                  ? "Pick a passphrase the cloud can't read. You'll use it to access your configs on other devices."
                  : 'Enter the sync password you created on your first device to decrypt configs here.'}
              </p>
            </div>
            <button className="btn btn-secondary" onClick={openSyncKeyDialog}>
              <KeyRound size={13} strokeWidth={2.2} />
              {syncKeyStatus === 'not_set_up' ? 'Set up' : 'Unlock'}
            </button>
          </div>
        )}

        {plan === 'free' ? (
          <div className="cloud-tiers">
            <TierCard
              name="Hobby" price="€5"
              features={['Cross-device sync', 'Email + Discord alerts', 'Encrypted backups']}
              busy={busy === 'hobby'}
              onClick={() => withBusy('hobby', () => openCheckout('hobby'))}
            />
            <TierCard
              name="Team" price="€12" featured
              features={['Everything in Hobby', 'Unlimited sub-users + RBAC', 'Audit log + relay']}
              busy={busy === 'team'}
              onClick={() => withBusy('team', () => openCheckout('team'))}
            />
          </div>
        ) : (
          <div className="cloud-paid">
            <div>
              <div className="eyebrow">Subscription</div>
              <div className="cloud-paid-detail">
                {cancelling ? (
                  <>Cancels on <strong>{formatDate(periodEnd!)}</strong>. Re-enable from the portal.</>
                ) : periodEnd ? (
                  <>Renews on <strong>{formatDate(periodEnd)}</strong>.</>
                ) : (
                  <>Active.</>
                )}
              </div>
            </div>
            <button
              className="btn btn-secondary"
              disabled={busy === 'portal'}
              onClick={() => withBusy('portal', () => openPortal())}
            >
              <ExternalLink size={13} strokeWidth={2.2} />
              {busy === 'portal' ? '…' : 'Manage billing'}
            </button>
          </div>
        )}

        <div className="cloud-foot">
          <ShieldCheck size={12} className="text-emerald-400" />
          <span>Server configs sync end-to-end encrypted. The cloud can't read them.</span>
          <button
            className="cloud-foot-link"
            onClick={() => withBusy('export', async () => {
              const path = await exportData();
              if (path) {
                console.log('[export] saved to', path);
              }
            })}
            disabled={busy === 'export'}
            title="Download a JSON of everything we hold for you"
          >
            <Download size={11} />
            {busy === 'export' ? '…' : 'Download my data'}
          </button>
        </div>
      </section>

      {/* Sync section — only meaningful for paid plans. The DB schema
          allows free users to read /v1/sync/* in theory but the cloud
          API 402s them, so we hide the section entirely to avoid
          showing a button that always errors. */}
      {plan !== 'free' && (
        <section className="card mb-5">
          <div className="section-header">
            <div className="section-title">
              <RefreshCw size={15} className="text-sky-400" />
              Cloud sync
            </div>
            <button
              className="btn btn-secondary"
              onClick={() => withBusy('sync', async () => { await syncNow(); })}
              disabled={syncing}
            >
              <RefreshCw size={13} strokeWidth={2.2} className={syncing ? 'animate-spin' : ''} />
              {syncing ? 'Syncing…' : 'Sync now'}
            </button>
          </div>

          {lastSyncedAt && (
            <div className="sync-meta">
              <span>Last synced {relativeTime(lastSyncedAt)}.</span>
              {lastSyncResult && (
                <span>
                  Pushed <strong>{lastSyncResult.pushed}</strong>, found
                  {' '}<strong>{lastSyncResult.remote.length}</strong> in cloud
                  {lastSyncResult.remote.filter((r) => !r.exists_locally).length > 0 && (
                    <> · <strong>{lastSyncResult.remote.filter((r) => !r.exists_locally).length}</strong> not on this device</>
                  )}
                  {lastSyncResult.conflicts.length > 0 && (
                    <> · <span className="text-amber-400">{lastSyncResult.conflicts.length} conflicts</span></>
                  )}
                </span>
              )}
            </div>
          )}

          {lastSyncResult && lastSyncResult.remote.some((r) => r.decrypt_error) && (
            <div className="cloud-warn mt-2">
              <AlertTriangle size={14} className="shrink-0 text-amber-400 mt-[2px]" />
              <span>
                Some cloud configs couldn't be decrypted with this device's recovery key.
                If you set up sync on another device first, restore the key here from <strong>Recovery key → Restore</strong>.
              </span>
            </div>
          )}

          <div className="vault-actions">
            <button className="btn btn-secondary" onClick={() => setKeyDialog('show')}>
              <KeyRound size={13} strokeWidth={2.2} />
              Show recovery key
            </button>
            <button className="btn btn-ghost" onClick={() => setKeyDialog('import')}>
              <Upload size={13} strokeWidth={2.2} />
              Restore from a key
            </button>
            <span className="sync-hint">
              <Download size={11} className="inline mr-1" />
              The raw key stays on this device — store it like a password. The
              cloud keeps only a copy encrypted behind your sync passphrase.
            </span>
          </div>
        </section>
      )}

      <RecoveryKeyDialog
        open={keyDialog !== null}
        mode={keyDialog ?? 'show'}
        onClose={() => setKeyDialog(null)}
      />
    </>
  );
}

function relativeTime(unixMs: number): string {
  const delta = Date.now() - unixMs;
  if (delta < 60_000) return 'just now';
  if (delta < 3_600_000) return `${Math.round(delta / 60_000)}m ago`;
  if (delta < 86_400_000) return `${Math.round(delta / 3_600_000)}h ago`;
  return `${Math.round(delta / 86_400_000)}d ago`;
}

function TierCard({
  name, price, features, featured, busy, onClick,
}: {
  name: string;
  price: string;
  features: string[];
  featured?: boolean;
  busy: boolean;
  onClick: () => void;
}) {
  return (
    <div className={`tier-card${featured ? ' tier-card-featured' : ''}`}>
      <div className="eyebrow">{name}</div>
      <div className="tier-price">{price}<span>/mo</span></div>
      <ul className="tier-features">
        {features.map((f) => <li key={f}>{f}</li>)}
      </ul>
      <button className={`btn btn-${featured ? 'primary' : 'secondary'} w-full`} onClick={onClick} disabled={busy}>
        {busy ? '…' : `Start ${name}`}
      </button>
    </div>
  );
}

function formatDate(unixMs: number): string {
  return new Date(unixMs).toLocaleDateString(undefined, {
    year: 'numeric', month: 'short', day: 'numeric',
  });
}
