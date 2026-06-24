/**
 * Settings page — split into sub-tabs so each concern has its own space.
 *
 *   Account        — cloud account, billing, sync key, org switcher
 *   Team           — members, invitations, roles (Team plan only)
 *   Backup Storage — org-level S3 target management
 *   System         — Docker integration, about, data locations
 *
 * Deep-link via ?tab=<id> so other parts of the app can navigate directly
 * to the right tab (e.g. BackupsPanel → "Configure backup storage").
 */
import { useEffect, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import {
  Activity, Cpu, HardDrive, Container, Box, Info,
  Folder, Archive, Users, UserCircle, Wrench, Webhook,
  RefreshCw,
} from 'lucide-react';
import { CloudAccountPanel } from '../components/CloudAccountPanel';
import { MembersPanel } from '../components/MembersPanel';
import { BackupStoragePanel } from '../components/BackupStoragePanel';
import { WebhooksPanel } from '../components/WebhooksPanel';
import { ActivityPanel } from '../components/ActivityPanel';
import { useDockerStore } from '../stores/dockerStore';

type SettingsTab = 'account' | 'team' | 'activity' | 'backup-storage' | 'alerts' | 'system';

const TABS: Array<{ id: SettingsTab; label: string; icon: React.ReactNode }> = [
  { id: 'account',        label: 'Account',        icon: <UserCircle size={15} /> },
  { id: 'team',           label: 'Team',            icon: <Users size={15} /> },
  { id: 'activity',       label: 'Activity',        icon: <Activity size={15} /> },
  { id: 'backup-storage', label: 'Backup storage',  icon: <Archive size={15} /> },
  { id: 'alerts',         label: 'Alerts',          icon: <Webhook size={15} /> },
  { id: 'system',         label: 'System',          icon: <Wrench size={15} /> },
];

export function Settings() {
  const [searchParams, setSearchParams] = useSearchParams();
  const initialTab = (searchParams.get('tab') as SettingsTab | null) ?? 'account';
  const [activeTab, setActiveTab] = useState<SettingsTab>(initialTab);

  // Keep the URL in sync so back-navigation works and deep-links land correctly.
  function selectTab(tab: SettingsTab) {
    setActiveTab(tab);
    setSearchParams(tab === 'account' ? {} : { tab });
  }

  // The URL is the single source of truth for the active tab: deep-links
  // (?tab=) AND the browser back/forward buttons both flow through here.
  // Defaulting a missing param to 'account' makes back-navigation revert
  // correctly. No stale closure (activeTab isn't read) and no loop
  // (setActiveTab doesn't mutate searchParams).
  useEffect(() => {
    setActiveTab((searchParams.get('tab') as SettingsTab | null) ?? 'account');
  }, [searchParams]);

  const { status, info, checkStatus, fetchInfo, isChecking } = useDockerStore();
  useEffect(() => {
    checkStatus();
    fetchInfo();
  }, [checkStatus, fetchInfo]);

  return (
    <div className="animate-fade-in max-w-3xl">
      <header className="page-header">
        <div className="eyebrow mb-2">Configuration</div>
        <h1 className="page-title">Settings</h1>
        <p className="page-subtitle">Manage your account, team, and LocalForge configuration.</p>
      </header>

      {/* Sub-tab bar */}
      <div className="flex gap-1 mb-6 border-b border-zinc-800 pb-0">
        {TABS.map((t) => (
          <button
            key={t.id}
            className={`flex items-center gap-1.5 px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors ${
              activeTab === t.id
                ? 'border-orange-400 text-white'
                : 'border-transparent text-zinc-500 hover:text-zinc-300'
            }`}
            onClick={() => selectTab(t.id)}
          >
            {t.icon} {t.label}
          </button>
        ))}
      </div>

      {/* ── Account ─────────────────────────────────────────────────────── */}
      {activeTab === 'account' && (
        <CloudAccountPanel />
      )}

      {/* ── Team ────────────────────────────────────────────────────────── */}
      {activeTab === 'team' && (
        <MembersPanel />
      )}

      {/* ── Activity (audit feed) ───────────────────────────────────────── */}
      {activeTab === 'activity' && (
        <ActivityPanel />
      )}

      {/* ── Backup Storage ──────────────────────────────────────────────── */}
      {activeTab === 'backup-storage' && (
        <BackupStoragePanel />
      )}

      {/* ── Alerts (crash webhooks) ─────────────────────────────────────── */}
      {activeTab === 'alerts' && (
        <WebhooksPanel />
      )}

      {/* ── System ──────────────────────────────────────────────────────── */}
      {activeTab === 'system' && (
        <div className="space-y-5">
          {/* Docker status */}
          <section className="card">
            <div className="section-header mb-4">
              <div className="section-title">
                <Activity size={15} className="text-emerald-400" />
                Docker integration
              </div>
              <button
                onClick={() => { checkStatus(); fetchInfo(); }}
                disabled={isChecking}
                className="btn btn-secondary btn-sm"
              >
                <RefreshCw size={13} className={isChecking ? 'animate-spin' : ''} />
                Refresh
              </button>
            </div>
            <div className="settings-row">
              <span className="settings-row-label">
                <span className={`status-dot ${status?.running ? 'status-running' : 'status-error'}`} />
                Status
              </span>
              <span className={status?.running ? 'text-emerald-400 font-medium' : 'text-red-400 font-medium'}>
                {status?.running ? 'Connected' : 'Disconnected'}
              </span>
            </div>
            {info && (
              <>
                <SettingsRow icon={<Box size={13} />} label="Docker version">
                  <span className="font-mono text-slate-200">{info.version}</span>
                </SettingsRow>
                <SettingsRow icon={<Box size={13} />} label="API version">
                  <span className="font-mono text-slate-200">{info.api_version}</span>
                </SettingsRow>
                <SettingsRow icon={<HardDrive size={13} />} label="Operating system">
                  <span className="text-slate-200">{info.os}</span>
                </SettingsRow>
                <SettingsRow icon={<Cpu size={13} />} label="Architecture">
                  <span className="font-mono text-slate-200">{info.arch}</span>
                </SettingsRow>
                <SettingsRow icon={<Container size={13} />} label="Running containers">
                  <span className="tabular-nums text-slate-200">
                    {info.containers_running}{' '}
                    <span className="text-slate-500">/ {info.containers_total}</span>
                  </span>
                </SettingsRow>
                <SettingsRow icon={<Box size={13} />} label="Images" last>
                  <span className="tabular-nums text-slate-200">{info.images}</span>
                </SettingsRow>
              </>
            )}
          </section>

          {/* About */}
          <section className="card">
            <div className="section-title mb-3">
              <Info size={15} className="text-orange-400" />
              About LocalForge
            </div>
            <div className="space-y-3 text-sm text-slate-400 leading-relaxed">
              <p>
                LocalForge makes it easy to run game servers on your own computer.
                No cloud required, no monthly fees, no complicated setup.
              </p>
              <p>
                Built with Tauri, React and Rust. Each game server runs in its own isolated Docker container.
              </p>
            </div>
            <div className="flex items-center gap-2 pt-4 mt-3 border-t border-[var(--color-border)]">
              <span className="eyebrow">Version</span>
              <span className="font-mono text-xs text-slate-300">v0.1.52</span>
            </div>
          </section>

          {/* Data location */}
          <section className="card">
            <div className="section-title mb-4">
              <Folder size={15} className="text-amber-400" />
              Data location
            </div>
            <div className="space-y-3">
              <div>
                <div className="text-[11px] uppercase tracking-wider font-semibold text-slate-500 mb-1">
                  Server data
                </div>
                <div className="font-mono text-[12.5px] text-slate-200 bg-[var(--color-surface)] border border-[var(--color-border)] rounded-md px-3 py-2">
                  ~/LocalForge/servers/
                </div>
              </div>
              <div>
                <div className="text-[11px] uppercase tracking-wider font-semibold text-slate-500 mb-1">
                  Configuration
                </div>
                <div className="font-mono text-[12.5px] text-slate-200 bg-[var(--color-surface)] border border-[var(--color-border)] rounded-md px-3 py-2">
                  ~/LocalForge/config/
                </div>
              </div>
            </div>
            <p className="text-xs text-slate-500 mt-4">
              World saves and configs persist even when you delete a server.
            </p>
          </section>
        </div>
      )}
    </div>
  );
}

function SettingsRow({
  icon, label, children, last,
}: {
  icon: React.ReactNode; label: string;
  children: React.ReactNode; last?: boolean;
}) {
  return (
    <div className={`settings-row ${last ? 'settings-row-last' : ''}`}>
      <span className="settings-row-label">
        <span className="text-slate-500">{icon}</span>
        {label}
      </span>
      <span className="text-[13.5px]">{children}</span>
    </div>
  );
}
