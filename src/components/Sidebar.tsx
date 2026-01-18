import { NavLink } from 'react-router-dom';
import { Home, Server, Plus, Settings, Activity, Gamepad2 } from 'lucide-react';
import { useServerStore } from '../stores/serverStore';
import { useDockerStore } from '../stores/dockerStore';
import { useGamesStore } from '../stores/gamesStore';

export function Sidebar() {
  const { servers } = useServerStore();
  const { status, info } = useDockerStore();
  const { games } = useGamesStore();
  
  const runningCount = servers.filter(s => s.status === 'running').length;
  const customGamesCount = games.filter(g => g.is_custom).length;

  return (
    <aside className="w-56 bg-slate-900/50 flex flex-col h-full border-r border-slate-800/50">
      {/* Navigation */}
      <nav className="flex-1 px-3 py-4 space-y-1">
        <NavLink
          to="/"
          className={({ isActive }) =>
            `flex items-center gap-3 px-3 py-2 rounded-lg transition-colors text-sm ${
              isActive
                ? 'bg-blue-600 text-white'
                : 'text-slate-400 hover:text-white hover:bg-slate-800'
            }`
          }
        >
          <Home size={18} />
          Dashboard
        </NavLink>

        <NavLink
          to="/servers"
          className={({ isActive }) =>
            `flex items-center gap-3 px-3 py-2 rounded-lg transition-colors text-sm ${
              isActive
                ? 'bg-blue-600 text-white'
                : 'text-slate-400 hover:text-white hover:bg-slate-800'
            }`
          }
        >
          <Server size={18} />
          My Servers
          {servers.length > 0 && (
            <span className="ml-auto bg-slate-800 text-slate-400 text-xs px-2 py-0.5 rounded">
              {servers.length}
            </span>
          )}
        </NavLink>

        <NavLink
          to="/servers/create"
          className={({ isActive }) =>
            `flex items-center gap-3 px-3 py-2 rounded-lg transition-colors text-sm ${
              isActive
                ? 'bg-blue-600 text-white'
                : 'text-slate-400 hover:text-white hover:bg-slate-800'
            }`
          }
        >
          <Plus size={18} />
          Create Server
        </NavLink>

        <NavLink
          to="/games"
          className={({ isActive }) =>
            `flex items-center gap-3 px-3 py-2 rounded-lg transition-colors text-sm ${
              isActive
                ? 'bg-blue-600 text-white'
                : 'text-slate-400 hover:text-white hover:bg-slate-800'
            }`
          }
        >
          <Gamepad2 size={18} />
          Game Templates
          {customGamesCount > 0 && (
            <span className="ml-auto bg-blue-500/20 text-blue-400 text-xs px-2 py-0.5 rounded">
              +{customGamesCount}
            </span>
          )}
        </NavLink>

        <NavLink
          to="/settings"
          className={({ isActive }) =>
            `flex items-center gap-3 px-3 py-2 rounded-lg transition-colors text-sm ${
              isActive
                ? 'bg-blue-600 text-white'
                : 'text-slate-400 hover:text-white hover:bg-slate-800'
            }`
          }
        >
          <Settings size={18} />
          Settings
        </NavLink>
      </nav>

      {/* Status Footer */}
      <div className="p-4 border-t border-slate-800/50">
        <div className="flex items-center gap-2 text-sm">
          <Activity size={14} className={status?.running ? 'text-emerald-500' : 'text-red-500'} />
          <span className="text-slate-500 text-xs">
            {status?.running ? 'Docker Connected' : 'Docker Offline'}
          </span>
        </div>
        {runningCount > 0 && (
          <div className="mt-2 text-xs text-slate-500">
            {runningCount} server{runningCount !== 1 ? 's' : ''} running
          </div>
        )}
        {info && (
          <div className="mt-1 text-xs text-slate-600">
            v{info.version}
          </div>
        )}
      </div>
    </aside>
  );
}
