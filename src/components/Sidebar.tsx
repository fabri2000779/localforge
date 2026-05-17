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
  const { servers } = useServerStore();
  const { status, info } = useDockerStore();
  const { games } = useGamesStore();
  const { nodes } = useNodesStore();

  const runningCount = servers.filter((s) => s.status === 'running').length;
  const customGamesCount = games.filter((g) => g.is_custom).length;
  const remoteCount = nodes.filter((n) => n.kind.kind === 'remote').length;

  return (
    <aside className="app-sidebar">
      {/* Active node selector */}
      <div className="px-3 pt-4 pb-3 border-b border-[var(--color-border)]">
        <div className="sidebar-section-label !mt-0 !mb-1.5 !px-1">
          Active node
        </div>
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
          {servers.length > 0 && (
            <span className="sidebar-badge">{servers.length}</span>
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
              status?.running ? 'status-running' : 'status-error'
            }`}
          />
          <span className="text-[11.5px] font-medium text-slate-300">
            {status?.running ? 'Docker connected' : 'Docker offline'}
          </span>
        </div>
        {runningCount > 0 && (
          <div className="flex items-center gap-1.5 text-[11px] text-slate-500">
            <Activity size={11} className="text-emerald-400" />
            {runningCount} server{runningCount !== 1 ? 's' : ''} running
          </div>
        )}
        {info && (
          <div className="text-[10.5px] text-slate-600 font-mono tracking-tight">
            Docker {info.version}
          </div>
        )}
      </div>
    </aside>
  );
}
