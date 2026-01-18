import { useNavigate } from 'react-router-dom';
import { Plus, Server, Play, Square } from 'lucide-react';
import { useServerStore } from '../stores/serverStore';
import { useGamesStore } from '../stores/gamesStore';
import { ServerCard } from '../components/ServerCard';
import { GameIcon } from '../components/GameIcon';

export function Home() {
  const navigate = useNavigate();
  const { servers } = useServerStore();
  const { games } = useGamesStore();

  const runningServers = servers.filter(s => s.status === 'running');
  const stoppedServers = servers.filter(s => s.status === 'stopped');

  return (
    <div className="animate-fade-in">
      <header className="mb-8">
        <h1 className="text-3xl font-bold">Welcome to Serverwave Anywhere</h1>
        <p className="text-slate-400 mt-2">
          Create and manage game servers with a single click
        </p>
      </header>

      {/* Quick Stats */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-8">
        <div className="card flex items-center gap-4">
          <div className="p-3 bg-blue-600/20 rounded-lg">
            <Server className="text-blue-400" size={24} />
          </div>
          <div>
            <div className="text-2xl font-bold">{servers.length}</div>
            <div className="text-sm text-slate-400">Total Servers</div>
          </div>
        </div>

        <div className="card flex items-center gap-4">
          <div className="p-3 bg-emerald-600/20 rounded-lg">
            <Play className="text-emerald-400" size={24} />
          </div>
          <div>
            <div className="text-2xl font-bold">{runningServers.length}</div>
            <div className="text-sm text-slate-400">Running</div>
          </div>
        </div>

        <div className="card flex items-center gap-4">
          <div className="p-3 bg-slate-600/20 rounded-lg">
            <Square className="text-slate-400" size={24} />
          </div>
          <div>
            <div className="text-2xl font-bold">{stoppedServers.length}</div>
            <div className="text-sm text-slate-400">Stopped</div>
          </div>
        </div>
      </div>

      {/* Running Servers */}
      {runningServers.length > 0 && (
        <section className="mb-8">
          <h2 className="text-xl font-semibold mb-4 flex items-center gap-2">
            <span className="status-dot status-running"></span>
            Running Servers
          </h2>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {runningServers.map(server => (
              <ServerCard key={server.id} server={server} />
            ))}
          </div>
        </section>
      )}

      {/* Quick Create */}
      {servers.length === 0 && (
        <section className="mb-8">
          <div className="card text-center py-12">
            <div className="text-5xl mb-4">🎮</div>
            <h2 className="text-xl font-semibold mb-2">No servers yet</h2>
            <p className="text-slate-400 mb-6">
              Create your first game server in seconds
            </p>
            <button
              onClick={() => navigate('/servers/create')}
              className="btn btn-primary"
            >
              <Plus size={20} />
              Create Server
            </button>
          </div>
        </section>
      )}

      {/* Available Games */}
      <section>
        <h2 className="text-xl font-semibold mb-4">Supported Games</h2>
        <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
          {games.map(game => (
            <div
              key={game.game_type}
              onClick={() => navigate('/servers/create', { state: { gameType: game.game_type } })}
              className="card cursor-pointer hover:border-blue-500 transition-colors"
            >
              <div className="mb-3">
                <GameIcon 
                  icon={game.icon} 
                  logoUrl={game.logo_url} 
                  name={game.name}
                  size="lg"
                />
              </div>
              <h3 className="font-semibold">{game.name}</h3>
              <p className="text-xs text-slate-500 mt-1 line-clamp-2">
                {game.description}
              </p>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
