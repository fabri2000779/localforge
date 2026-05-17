import { NavLink } from 'react-router-dom';
import { Home, Server, Plus, Settings, Activity, Gamepad2, Cloud } from 'lucide-react';
import { useServerStore } from '../stores/serverStore';
import { useDockerStore } from '../stores/dockerStore';
import { useGamesStore } from '../stores/gamesStore';
import { useNodesStore } from '../stores/nodesStore';
import { NodeSelector } from './NodeSelector';

const itemBase =
  'flex items-center gap-3 px-3 py-2.5 rounded-lg transition-colors text-sm';
const itemActive = 'bg-blue-600 text-white shadow-sm shadow-blue-600/30';
const itemIdle = 'text-slate-400 hover:text-white hover:bg-slate-800/70';

function navClass({ isActive }: { isActive: boolean }) {
  return `${itemBase} ${isActive ? itemActive : itemIdle}`;
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
    <aside className="w-64 bg-slate-900/40 flex flex-col h-full border-r border-slate-800/60 shrink-0">
      {/* Active node selector */}
      <div className="px-3 pt-4 pb-3 border-b border-slate-800/60">
        <NodeSelector />
      </div>

      {/* Navigation */}
      <nav className="flex-1 px-2 py-4 space-y-0.5 overflow-y-auto">
        <NavLink to="/" end className={navClass}>
          <Home size={18} className="shrink-0" />
          <span className="flex-1">Dashboard</span>
        </NavLink>

        <NavLink to="/servers" className={navClass}>
          <Server size={18} className="shrink-0" />
          <span className="flex-1">My Servers</span>
          {servers.length > 0 && (
            <span className="bg-slate-800 text-slate-300 text-xs px-2 py-0.5 rounded-full">
              {servers.length}
            </span>
          )}
        </NavLink>

        <NavLink to="/servers/create" className={navClass}>
          <Plus size={18} className="shrink-0" />
          <span className="flex-1">Create Server</span>
        </NavLink>

        <NavLink to="/games" className={navClass}>
          <Gamepad2 size={18} className="shrink-0" />
          <span className="flex-1">Game Templates</span>
          {customGamesCount > 0 && (
            <span className="bg-blue-500/15 text-blue-400 text-xs px-2 py-0.5 rounded-full">
              +{customGamesCount}
            </span>
          )}
        </NavLink>

        <NavLink to="/nodes" className={navClass}>
          <Cloud size={18} className="shrink-0" />
          <span className="flex-1">Nodes</span>
          {remoteCount > 0 && (
            <span className="bg-purple-500/15 text-purple-400 text-xs px-2 py-0.5 rounded-full">
              +{remoteCount}
            </span>
          )}
        </NavLink>

        <NavLink to="/settings" className={navClass}>
          <Settings size={18} className="shrink-0" />
          <span className="flex-1">Settings</span>
        </NavLink>
      </nav>

      {/* Status Footer */}
      <div className="p-4 border-t border-slate-800/60 space-y-1.5">
        <div className="flex items-center gap-2">
          <Activity
            size={14}
            className={status?.running ? 'text-emerald-500' : 'text-red-500'}
          />
          <span className="text-xs text-slate-400">
            {status?.running ? 'Docker connected' : 'Docker offline'}
          </span>
        </div>
        {runningCount > 0 && (
          <div className="text-xs text-slate-500">
            {runningCount} server{runningCount !== 1 ? 's' : ''} running
          </div>
        )}
        {info && <div className="text-xs text-slate-600">v{info.version}</div>}
      </div>
    </aside>
  );
}
