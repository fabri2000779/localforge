import { NavLink } from 'react-router-dom';
import {
  Home,
  Server,
  Plus,
  Settings,
  Activity,
  Gamepad2,
  Cloud,
} from 'lucide-react';
import { useServerStore } from '../stores/serverStore';
import { useDockerStore } from '../stores/dockerStore';
import { useGamesStore } from '../stores/gamesStore';
import { useNodesStore } from '../stores/nodesStore';
import { NodeSelector } from './NodeSelector';

function navClass({ isActive }: { isActive: boolean }) {
  return `sidebar-item ${isActive ? 'active' : ''}`;
}

export function Sidebar() {
  const serverCount = useServerStore((s) => s.servers.length);
  const runningCount = useServerStore((s) => s.servers.filter((server) => server.status === 'running').length);
  const dockerRunning = useDockerStore((s) => s.status?.running);
  const dockerVersion = useDockerStore((s) => s.info?.version);
  const customGamesCount = useGamesStore((s) => s.games.filter((game) => game.is_custom).length);
  const remoteCount = useNodesStore((s) => s.nodes.filter((node) => node.kind.kind === 'remote').length);

  return (
    <aside className="app-sidebar">
      {/* Active node selector — has its own "Active node" label inside */}
      <div className="px-3 pt-4 pb-3 border-b border-[var(--color-border)]">
        <NodeSelector />
      </div>

      {/* Navigation */}
      <nav className="sidebar-nav">
        <div className="sidebar-section-label !mt-1">Workspace</div>

        <NavLink to="/" end className={navClass}>
          <Home size={16} strokeWidth={2} className="shrink-0" />
          <span className="flex-1">Dashboard</span>
        </NavLink>

        <NavLink to="/servers" className={navClass}>
          <Server size={16} strokeWidth={2} className="shrink-0" />
          <span className="flex-1">My Servers</span>
          {serverCount > 0 && (
            <span className="sidebar-badge">{serverCount}</span>
          )}
        </NavLink>

        <NavLink to="/servers/create" className={navClass}>
          <Plus size={16} strokeWidth={2} className="shrink-0" />
          <span className="flex-1">Create Server</span>
        </NavLink>

        <div className="sidebar-section-label">Library</div>

        <NavLink to="/games" className={navClass}>
          <Gamepad2 size={16} strokeWidth={2} className="shrink-0" />
          <span className="flex-1">Game Templates</span>
          {customGamesCount > 0 && (
            <span className="sidebar-badge sidebar-badge-accent">
              +{customGamesCount}
            </span>
          )}
        </NavLink>

        <NavLink to="/nodes" className={navClass}>
          <Cloud size={16} strokeWidth={2} className="shrink-0" />
          <span className="flex-1">Nodes</span>
          {remoteCount > 0 && (
            <span className="sidebar-badge sidebar-badge-purple">
              +{remoteCount}
            </span>
          )}
        </NavLink>

        <div className="sidebar-section-label">System</div>

        <NavLink to="/settings" className={navClass}>
          <Settings size={16} strokeWidth={2} className="shrink-0" />
          <span className="flex-1">Settings</span>
        </NavLink>
      </nav>

      {/* Status Footer */}
      <div className="sidebar-footer">
        <div className="flex items-center gap-2">
          <span
            className={`status-dot ${
              dockerRunning ? 'status-running' : 'status-error'
            }`}
          />
          <span className="text-[11.5px] font-medium text-slate-300">
            {dockerRunning ? 'Docker connected' : 'Docker offline'}
          </span>
        </div>
        {runningCount > 0 && (
          <div className="flex items-center gap-1.5 text-[11px] text-slate-500">
            <Activity size={11} className="text-emerald-400" />
            {runningCount} server{runningCount !== 1 ? 's' : ''} running
          </div>
        )}
        {dockerVersion != null && (
          <div className="text-[10.5px] text-slate-600 font-mono tracking-tight">
            Docker {dockerVersion}
          </div>
        )}
      </div>
    </aside>
  );
}
