import { useEffect } from 'react';
import { useDockerStore } from '../stores/dockerStore';
import { RefreshCw, ExternalLink } from 'lucide-react';

export function Settings() {
  const { status, info, checkStatus, fetchInfo, isChecking } = useDockerStore();

  useEffect(() => {
    checkStatus();
    fetchInfo();
  }, []);

  return (
    <div className="animate-fade-in max-w-3xl">
      <header className="mb-8">
        <h1 className="text-3xl font-bold">Settings</h1>
        <p className="text-slate-400 mt-2">
          Manage Serverwave Anywhere configuration
        </p>
      </header>

      {/* Docker Status */}
      <section className="card mb-6">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-xl font-semibold">Docker Status</h2>
          <button
            onClick={() => { checkStatus(); fetchInfo(); }}
            disabled={isChecking}
            className="btn btn-secondary text-sm"
          >
            <RefreshCw size={16} className={isChecking ? 'animate-spin' : ''} />
            Refresh
          </button>
        </div>

        <div className="space-y-4">
          <div className="flex items-center justify-between py-2 border-b border-slate-700">
            <span className="text-slate-400">Status</span>
            <span className={status?.running ? 'text-emerald-500' : 'text-red-500'}>
              {status?.running ? '● Connected' : '● Disconnected'}
            </span>
          </div>

          {info && (
            <>
              <div className="flex items-center justify-between py-2 border-b border-slate-700">
                <span className="text-slate-400">Docker Version</span>
                <span>{info.version}</span>
              </div>
              <div className="flex items-center justify-between py-2 border-b border-slate-700">
                <span className="text-slate-400">API Version</span>
                <span>{info.api_version}</span>
              </div>
              <div className="flex items-center justify-between py-2 border-b border-slate-700">
                <span className="text-slate-400">Operating System</span>
                <span>{info.os}</span>
              </div>
              <div className="flex items-center justify-between py-2 border-b border-slate-700">
                <span className="text-slate-400">Architecture</span>
                <span>{info.arch}</span>
              </div>
              <div className="flex items-center justify-between py-2 border-b border-slate-700">
                <span className="text-slate-400">Running Containers</span>
                <span>{info.containers_running} / {info.containers_total}</span>
              </div>
              <div className="flex items-center justify-between py-2">
                <span className="text-slate-400">Images</span>
                <span>{info.images}</span>
              </div>
            </>
          )}
        </div>
      </section>

      {/* About */}
      <section className="card mb-6">
        <h2 className="text-xl font-semibold mb-4">About Serverwave Anywhere</h2>
        <div className="space-y-4 text-slate-400">
          <p>
            Serverwave Anywhere makes it easy to run game servers on your own computer.
            No cloud required, no monthly fees, no complicated setup.
          </p>
          <p>
            Built with Tauri, React, and Rust. Powered by Docker.
          </p>
          <div className="flex items-center gap-4 pt-4">
            <a
              href="https://serverwave.com"
              target="_blank"
              rel="noopener noreferrer"
              className="btn btn-secondary text-sm"
            >
              <ExternalLink size={16} />
              Serverwave
            </a>
            <span className="text-sm text-slate-600">v0.1.0</span>
          </div>
        </div>
      </section>

      {/* Data Location */}
      <section className="card">
        <h2 className="text-xl font-semibold mb-4">Data Location</h2>
        <div className="space-y-3 text-sm">
          <div>
            <span className="text-slate-500">Server Data:</span>
            <div className="font-mono text-slate-300 mt-1">~/ServerWaveAnywhere/servers/</div>
          </div>
          <div>
            <span className="text-slate-500">Configuration:</span>
            <div className="font-mono text-slate-300 mt-1">~/ServerWaveAnywhere/config/</div>
          </div>
        </div>
        <p className="text-xs text-slate-500 mt-4">
          World saves and configs persist even when you delete a server.
        </p>
      </section>
    </div>
  );
}
