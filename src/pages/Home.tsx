import { useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  Plus,
  Server,
  Play,
  Square,
  Cloud,
  Wifi,
  ArrowRight,
  Sparkles,
} from 'lucide-react';
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
      {/* Hero */}
      <header className="page-header flex items-start justify-between gap-6 flex-wrap">
        <div>
          <div className="eyebrow mb-2 flex items-center gap-1.5">
            <Sparkles size={11} className="text-blue-400" />
            Dashboard
          </div>
          <h1 className="page-title">Welcome to LocalForge</h1>
          <p className="page-subtitle max-w-xl">
            Spin up, manage and observe your game servers — locally or across
            remote nodes — without leaving the desktop.
          </p>
        </div>
        <button
          onClick={() => navigate('/servers/create')}
          className="btn btn-primary"
        >
          <Plus size={16} strokeWidth={2.2} />
          New server
        </button>
      </header>

      {/* Quick Stats */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 mb-8">
        <StatCard
          label="Total servers"
          value={servers.length}
          icon={<Server size={18} className="text-blue-300" />}
          accent="rgba(59, 130, 246, 0.14)"
        />
        <StatCard
          label="Running"
          value={runningServers.length}
          icon={<Play size={18} className="text-emerald-300" />}
          accent="rgba(34, 197, 94, 0.14)"
        />
        <StatCard
          label="Stopped"
          value={stoppedServers.length}
          icon={<Square size={18} className="text-slate-400" />}
          accent="rgba(148, 163, 184, 0.10)"
        />
      </div>

      {/* Cluster overview — only meaningful once a remote agent is paired */}
      {remoteNodes.length > 0 && clusterSummary && (
        <section className="card card-elevated mb-8">
          <div className="section-header !mb-5">
            <div className="section-title">
              <Cloud size={15} className="text-purple-400" />
              Across all nodes
            </div>
            <button
              onClick={() => navigate('/nodes')}
              className="text-[11.5px] text-slate-500 hover:text-slate-200 flex items-center gap-1 transition-colors"
            >
              Manage nodes
              <ArrowRight size={11} />
            </button>
          </div>
          <div className="grid grid-cols-2 md:grid-cols-4 gap-5">
            <ClusterStat
              label="Nodes online"
              value={`${clusterSummary.online_nodes}/${clusterSummary.total_nodes}`}
              icon={<Wifi size={13} className="text-emerald-400" />}
            />
            <ClusterStat
              label="Containers running"
              value={clusterSummary.containers_running.toString()}
              icon={<Play size={13} className="text-emerald-400" />}
            />
            <ClusterStat
              label="Containers total"
              value={clusterSummary.containers_total.toString()}
              icon={<Server size={13} className="text-blue-400" />}
            />
            <ClusterStat
              label="Images"
              value={clusterSummary.images.toString()}
              icon={<Square size={13} className="text-slate-400" />}
            />
          </div>
        </section>
      )}

      {/* Running Servers */}
      {runningServers.length > 0 && (
        <section className="mb-10">
          <div className="section-header">
            <div className="section-title">
              <span className="status-dot status-running" />
              Running servers
            </div>
            {runningServers.length > 2 && (
              <button
                onClick={() => navigate('/servers')}
                className="text-[11.5px] text-slate-500 hover:text-slate-200 flex items-center gap-1 transition-colors"
              >
                View all
                <ArrowRight size={11} />
              </button>
            )}
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {runningServers.map((server) => (
              <ServerCard key={server.id} server={server} />
            ))}
          </div>
        </section>
      )}

      {/* Empty state */}
      {servers.length === 0 && (
        <section className="mb-10">
          <div className="card card-elevated text-center py-14 px-6 flex flex-col items-center">
            <div
              className="w-14 h-14 rounded-2xl flex items-center justify-center mb-5"
              style={{
                background:
                  'linear-gradient(135deg, rgba(59,130,246,0.18), rgba(139,92,246,0.18))',
                boxShadow:
                  '0 0 0 1px rgba(99, 102, 241, 0.18), 0 14px 30px -10px rgba(59,130,246,0.35)',
              }}
            >
              <Server size={24} className="text-blue-200" />
            </div>
            <h2 className="text-lg font-semibold text-slate-100 mb-1.5">
              No servers yet
            </h2>
            <p className="text-sm text-slate-400 mb-6 max-w-sm">
              Choose a game template below to spin up your first server. Most
              games are ready to play in under a minute.
            </p>
            <button
              onClick={() => navigate('/servers/create')}
              className="btn btn-primary"
            >
              <Plus size={16} strokeWidth={2.2} />
              Create your first server
            </button>
          </div>
        </section>
      )}

      {/* Available Games */}
      <section>
        <div className="section-header">
          <div className="section-title">Supported games</div>
          <span className="text-[11.5px] text-slate-500">
            {games.length} templates
          </span>
        </div>
        <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-3">
          {games.map((game) => (
            <button
              key={game.game_type}
              onClick={() =>
                navigate('/servers/create', {
                  state: { gameType: game.game_type },
                })
              }
              className="game-tile group"
            >
              <GameIcon
                icon={game.icon}
                logoUrl={game.logo_url}
                name={game.name}
                size="lg"
                className="mb-3"
              />
              <h3 className="font-semibold text-[13.5px] leading-tight text-slate-100">
                {game.name}
              </h3>
              <p className="text-[11.5px] text-slate-500 mt-1.5 line-clamp-2 leading-snug">
                {game.description}
              </p>
              <span className="game-tile-cta">
                Create
                <ArrowRight size={11} />
              </span>
            </button>
          ))}
        </div>
      </section>
    </div>
  );
}

function StatCard({
  label,
  value,
  icon,
  accent,
}: {
  label: string;
  value: number;
  icon: React.ReactNode;
  accent: string;
}) {
  return (
    <div className="stat-card">
      <div className="stat-icon" style={{ background: accent }}>
        {icon}
      </div>
      <div className="flex-1 min-w-0">
        <div className="stat-value tabular-nums">{value}</div>
        <div className="stat-label">{label}</div>
      </div>
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
      <div className="w-8 h-8 rounded-lg bg-slate-800/70 inline-flex items-center justify-center shrink-0">
        {icon}
      </div>
      <div className="min-w-0">
        <div className="text-base font-semibold text-slate-100 tabular-nums leading-tight">
          {value}
        </div>
        <div className="text-[10.5px] uppercase tracking-wider text-slate-500 font-medium mt-0.5">
          {label}
        </div>
      </div>
    </div>
  );
}
