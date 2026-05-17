import { useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { Plus, Server, Play, Square, Cloud, Wifi } from 'lucide-react';
import { useServerStore } from '../stores/serverStore';
import { useGamesStore } from '../stores/gamesStore';
import { useNodesStore } from '../stores/nodesStore';
import { ServerCard } from '../components/ServerCard';
import { GameIcon } from '../components/GameIcon';

export function Home() {
  const navigate = useNavigate();
  const { servers } = useServerStore();
  const { games } = useGamesStore();
  const { nodes, clusterSummary, fetchNodes, fetchClusterSummary } =
    useNodesStore();

  const runningServers = servers.filter((s) => s.status === 'running');
  const stoppedServers = servers.filter((s) => s.status === 'stopped');
  const remoteNodes = nodes.filter((n) => n.kind.kind === 'remote');

  useEffect(() => {
    fetchNodes();
    fetchClusterSummary();
    const interval = setInterval(fetchClusterSummary, 10_000);
    return () => clearInterval(interval);
  }, [fetchNodes, fetchClusterSummary]);

  return (
    <div className="animate-fade-in">
      <header className="mb-8">
        <h1 className="text-3xl font-bold">Welcome to LocalForge</h1>
        <p className="text-slate-400 mt-2">
          Create and manage game servers with a single click
        </p>
      </header>

      {/* Quick Stats (active node) */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-4">
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

      {/* Cluster overview — only meaningful once a remote agent is paired */}
      {remoteNodes.length > 0 && clusterSummary && (
        <div className="card mb-8">
          <div className="flex items-center justify-between mb-4">
            <div className="flex items-center gap-2">
              <Cloud className="text-purple-400" size={18} />
              <h2 className="text-lg font-semibold">Across all nodes</h2>
            </div>
            <button
              onClick={() => navigate('/nodes')}
              className="text-xs text-slate-500 hover:text-slate-300"
            >
              Manage nodes →
            </button>
          </div>
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
            <ClusterStat
              label="Nodes online"
              value={`${clusterSummary.online_nodes}/${clusterSummary.total_nodes}`}
              icon={<Wifi size={14} className="text-emerald-400" />}
            />
            <ClusterStat
              label="Containers running"
              value={clusterSummary.containers_running.toString()}
              icon={<Play size={14} className="text-emerald-400" />}
            />
            <ClusterStat
              label="Containers total"
              value={clusterSummary.containers_total.toString()}
              icon={<Server size={14} className="text-blue-400" />}
            />
            <ClusterStat
              label="Images"
              value={clusterSummary.images.toString()}
              icon={<Square size={14} className="text-slate-400" />}
            />
          </div>
        </div>
      )}

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
          <div className="card flex flex-col items-center text-center py-12">
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
        <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-3">
          {games.map(game => (
            <div
              key={game.game_type}
              onClick={() => navigate('/servers/create', { state: { gameType: game.game_type } })}
              className="card cursor-pointer hover:border-blue-500/60 hover:bg-slate-800/40 transition-colors !p-4 flex flex-col"
            >
              <GameIcon
                icon={game.icon}
                logoUrl={game.logo_url}
                name={game.name}
                size="lg"
                className="mb-3"
              />
              <h3 className="font-semibold text-sm leading-tight">{game.name}</h3>
              <p className="text-xs text-slate-500 mt-1.5 line-clamp-2 leading-snug">
                {game.description}
              </p>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}

function ClusterStat({
  label,
  value,
  icon,
}: {
  label: string;
  value: string;
  icon: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-3">
      <div className="p-2 rounded-lg bg-slate-800">{icon}</div>
      <div>
        <div className="text-lg font-semibold">{value}</div>
        <div className="text-xs text-slate-500">{label}</div>
      </div>
    </div>
  );
}
