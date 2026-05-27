import { useEffect } from 'react';
import { useDockerStore } from '../stores/dockerStore';
import { CloudAccountPanel } from '../components/CloudAccountPanel';
import { MembersPanel } from '../components/MembersPanel';
import {
  RefreshCw,
  Activity,
  Cpu,
  HardDrive,
  Container,
  Box,
  Info,
  Folder,
} from 'lucide-react';

export function Settings() {
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
        <p className="page-subtitle">
          Manage LocalForge configuration and inspect host integration.
        </p>
      </header>

      {/* Cloud account / billing — sign-in is OPTIONAL, this panel
          gracefully renders an empty state when nobody's signed in. */}
      <CloudAccountPanel />

      {/* Team panel only renders when the caller is on the Team plan;
          it's a no-op for free + hobby + signed-out users. */}
      <MembersPanel />

      {/* Docker Status */}
      <section className="card mb-5">
        <div className="section-header">
          <div className="section-title">
            <Activity size={15} className="text-emerald-400" />
            Docker integration
          </div>
          <button
            onClick={() => {
              checkStatus();
              fetchInfo();
            }}
            disabled={isChecking}
            className="btn btn-secondary btn-sm"
          >
            <RefreshCw
              size={13}
              className={isChecking ? 'animate-spin' : ''}
            />
            Refresh
          </button>
        </div>

        <div className="settings-row">
          <span className="settings-row-label">
            <span
              className={`status-dot ${
                status?.running ? 'status-running' : 'status-error'
              }`}
            />
            Status
          </span>
          <span
            className={
              status?.running
                ? 'text-emerald-400 font-medium'
                : 'text-red-400 font-medium'
            }
          >
            {status?.running ? 'Connected' : 'Disconnected'}
          </span>
        </div>

        {info && (
          <>
            <SettingsRow icon={<Box size={13} />} label="Docker version">
              <span className="font-mono text-slate-200">{info.version}</span>
            </SettingsRow>
            <SettingsRow icon={<Box size={13} />} label="API version">
              <span className="font-mono text-slate-200">
                {info.api_version}
              </span>
            </SettingsRow>
            <SettingsRow icon={<HardDrive size={13} />} label="Operating system">
              <span className="text-slate-200">{info.os}</span>
            </SettingsRow>
            <SettingsRow icon={<Cpu size={13} />} label="Architecture">
              <span className="font-mono text-slate-200">{info.arch}</span>
            </SettingsRow>
            <SettingsRow
              icon={<Container size={13} />}
              label="Running containers"
            >
              <span className="tabular-nums text-slate-200">
                {info.containers_running}{' '}
                <span className="text-slate-500">
                  / {info.containers_total}
                </span>
              </span>
            </SettingsRow>
            <SettingsRow icon={<Box size={13} />} label="Images" last>
              <span className="tabular-nums text-slate-200">{info.images}</span>
            </SettingsRow>
          </>
        )}
      </section>

      {/* About */}
      <section className="card mb-5">
        <div className="section-title mb-3">
          <Info size={15} className="text-blue-400" />
          About LocalForge
        </div>
        <div className="space-y-3 text-sm text-slate-400 leading-relaxed">
          <p>
            LocalForge makes it easy to run game servers on your own computer.
            No cloud required, no monthly fees, no complicated setup.
          </p>
          <p>
            Built with Tauri, React and Rust. Each game server runs in its
            own isolated Docker container.
          </p>
        </div>
        <div className="flex items-center gap-2 pt-4 mt-3 border-t border-[var(--color-border)]">
          <span className="eyebrow">Version</span>
          <span className="font-mono text-xs text-slate-300">v0.1.43</span>
        </div>
      </section>

      {/* Data Location */}
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
  );
}

function SettingsRow({
  icon,
  label,
  children,
  last,
}: {
  icon: React.ReactNode;
  label: string;
  children: React.ReactNode;
  last?: boolean;
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
