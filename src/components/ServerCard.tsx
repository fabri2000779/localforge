import { useNavigate } from 'react-router-dom';
import { Play, Square, Trash2, Terminal } from 'lucide-react';
import type { Server } from '../types';
import { useServerStore } from '../stores/serverStore';
import { useGamesStore } from '../stores/gamesStore';
import { findGameConfig } from '../utils/gameTypes';
import { GameIcon } from './GameIcon';

interface Props {
  server: Server;
}

export function ServerCard({ server }: Props) {
  const navigate = useNavigate();
  const { startServer, stopServer, deleteServer, isLoading } = useServerStore();
  const { games } = useGamesStore();
  
  const gameConfig = findGameConfig(games, server.game_type);

  const handleStart = async (e: React.MouseEvent) => {
    e.stopPropagation();
    await startServer(server.id);
  };

  const handleStop = async (e: React.MouseEvent) => {
    e.stopPropagation();
    await stopServer(server.id);
  };

  const handleDelete = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (confirm(`Delete server "${server.name}"? Your world data will be preserved.`)) {
      await deleteServer(server.id);
    }
  };

  const statusColors = {
    running: 'bg-emerald-500',
    stopped: 'bg-slate-500',
    starting: 'bg-yellow-500',
    stopping: 'bg-yellow-500',
    installing: 'bg-blue-500',
    error: 'bg-red-500',
  };

  const statusDot = statusColors[server.status] || statusColors.stopped;
  const isAnimated = server.status === 'running' || server.status === 'starting';

  return (
    <div
      onClick={() => navigate(`/servers/${server.id}`)}
      className="card cursor-pointer hover:border-slate-600 transition-all group"
    >
      <div className="flex items-start justify-between mb-4">
        <div className="flex items-center gap-3">
          <GameIcon 
            icon={gameConfig?.icon || '🎮'} 
            logoUrl={gameConfig?.logo_url}
            name={gameConfig?.name || server.game_type}
            size="lg"
          />
          <div>
            <h3 className="font-semibold text-lg">{server.name}</h3>
            <p className="text-sm text-slate-500">{gameConfig?.name || server.game_type}</p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <span className={`w-2.5 h-2.5 rounded-full ${statusDot} ${isAnimated ? 'animate-pulse' : ''}`}></span>
          <span className="text-sm text-slate-400 capitalize">{server.status}</span>
        </div>
      </div>

      <div className="flex items-center gap-2 text-sm text-slate-500 mb-4">
        <Terminal size={14} />
        <span>Port {server.port}</span>
      </div>

      <div className="flex items-center gap-2 opacity-0 group-hover:opacity-100 transition-opacity">
        {server.status === 'stopped' ? (
          <button
            onClick={handleStart}
            disabled={isLoading}
            className="btn btn-success text-sm py-1"
          >
            <Play size={16} />
            Start
          </button>
        ) : server.status === 'running' ? (
          <button
            onClick={handleStop}
            disabled={isLoading}
            className="btn btn-secondary text-sm py-1"
          >
            <Square size={16} />
            Stop
          </button>
        ) : null}

        <button
          onClick={handleDelete}
          disabled={isLoading || server.status === 'running'}
          className="btn btn-danger text-sm py-1 ml-auto"
        >
          <Trash2 size={16} />
        </button>
      </div>
    </div>
  );
}
