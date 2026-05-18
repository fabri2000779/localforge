/**
 * Account / billing panel embedded in the Settings page. Renders one of
 * three states based on auth + plan:
 *
 *   - Signed out  → tagline + "Sign in" button
 *   - Signed in, free → plan card with "Upgrade" CTAs (Hobby + Team)
 *   - Signed in, paid → plan card with "Manage billing" (opens portal)
 *
 * No screen-changing happens for billing actions — checkout + portal
 * both open in the system browser (the user pays / cancels there, then
 * comes back to the desktop; Stripe webhooks update our cloud state and
 * the app picks it up via cloud_me).
 */
import { useEffect, useState } from 'react';
import { Cloud, ExternalLink, Mail, ShieldCheck, AlertTriangle, LogOut } from 'lucide-react';
import { useAuthStore } from '../stores/authStore';
import { LoginDialog } from './LoginDialog';

export function CloudAccountPanel() {
  const me = useAuthStore((s) => s.me);
  const loading = useAuthStore((s) => s.loading);
  const refreshMe = useAuthStore((s) => s.refreshMe);
  const openCheckout = useAuthStore((s) => s.openCheckout);
  const openPortal = useAuthStore((s) => s.openPortal);
  const resendVerification = useAuthStore((s) => s.resendVerification);
  const logout = useAuthStore((s) => s.logout);

  const [loginOpen, setLoginOpen] = useState(false);
  const [resendOk, setResendOk] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  // Re-pull /me when the panel mounts so the displayed plan is fresh
  // (the user might have just upgraded in a browser tab and come back).
  useEffect(() => { void refreshMe(); }, [refreshMe]);

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
          <button className="btn-primary mt-3" disabled={loading} onClick={() => setLoginOpen(true)}>
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
  const planLabel = plan[0]!.toUpperCase() + plan.slice(1);

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
          <button className="btn-ghost" onClick={() => void logout()} title="Sign out">
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
              className="btn-secondary"
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
              className="btn-primary"
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
        </div>
      </section>
    </>
  );
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
      <button className={`btn-${featured ? 'primary' : 'secondary'} w-full`} onClick={onClick} disabled={busy}>
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
