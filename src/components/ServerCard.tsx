import { useNavigate } from 'react-router-dom';
import { Play, Square, Trash2, Network, ArrowRight, Cloud, Server as ServerIcon } from 'lucide-react';
import type { Server } from '../types';
import { useServerStore } from '../stores/serverStore';
import { useGamesStore } from '../stores/gamesStore';
import { findGameConfig } from '../utils/gameTypes';
import { GameIcon } from './GameIcon';

interface Props {
  server: Server;
  /** When the visible servers span more than one machine, the card shows
   *  which machine hosts this one. Undefined → no label (single-machine
   *  view, e.g. a solo owner). */
  machineLabel?: string;
  machineKind?: 'desktop' | 'agent' | 'local' | 'unknown';
  /** Override the "open detail" behaviour. The cross-machine fleet view
   *  uses this to first switch the active node to the server's machine so
   *  the detail screen resolves it. Default: navigate to /servers/:id. */
  onOpen?: () => void;
  /** Override start/stop/delete so they target the server's OWN node rather
   *  than the globally-active one. Default: the serverStore actions (which
   *  act on the active node / relay-route for sub-users). */
  onAction?: (action: 'start' | 'stop' | 'delete') => void;
}

const STATUS_LABEL: Record<Server['status'], string> = {
  running: 'Running',
  stopped: 'Stopped',
  starting: 'Starting',
  stopping: 'Stopping',
  installing: 'Installing',
  error: 'Error',
};

const STATUS_CLASS: Record<Server['status'], string> = {
  running: 'status-running',
  stopped: 'status-stopped',
  starting: 'status-starting',
  stopping: 'status-starting',
  installing: 'status-starting',
  error: 'status-error',
};

export function ServerCard({ server, machineLabel, machineKind, onOpen, onAction }: Props) {
  const navigate = useNavigate();
  const { startServer, stopServer, deleteServer, isLoading } = useServerStore();
  const { games } = useGamesStore();

  const gameConfig = findGameConfig(games, server.game_type);

  const open = () => {
    if (onOpen) onOpen();
    else navigate(`/servers/${server.id}`);
  };

  const handleStart = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (onAction) onAction('start');
    else await startServer(server.id);
  };

  const handleStop = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (onAction) onAction('stop');
    else await stopServer(server.id);
  };

  const handleDelete = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (
      confirm(
        `Delete server "${server.name}"? Your world data will be preserved.`,
      )
    ) {
      if (onAction) onAction('delete');
      else await deleteServer(server.id);
    }
  };

  return (
    <div
      onClick={open}
      className="server-card group"
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') open();
      }}
    >
      <div className="flex items-start gap-3.5">
        <GameIcon
          icon={gameConfig?.icon || '🎮'}
          logoUrl={gameConfig?.logo_url}
          name={gameConfig?.name || server.game_type}
          size="lg"
        />
        <div className="flex-1 min-w-0">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0">
              <h3 className="font-semibold text-[15px] text-slate-100 truncate leading-tight">
                {server.name}
              </h3>
              <p className="text-[12px] text-slate-500 mt-0.5 truncate">
                {gameConfig?.name || server.game_type}
              </p>
            </div>
            <div className="flex items-center gap-1.5 shrink-0">
              <span className={`status-dot ${STATUS_CLASS[server.status]}`} />
              <span className="text-[11px] uppercase tracking-wider font-semibold text-slate-400">
                {STATUS_LABEL[server.status]}
              </span>
            </div>
          </div>

          <div className="flex items-center gap-3 text-[12px] text-slate-500 mt-3">
            <span className="inline-flex items-center gap-1.5">
              <Network size={12} className="text-slate-600" />
              <span className="font-mono tabular-nums">:{server.port}</span>
            </span>
            {machineLabel && (
              <span className="inline-flex items-center gap-1.5 min-w-0" title={machineLabel}>
                {machineKind === 'agent' ? (
                  <Cloud size={12} className="text-slate-600 shrink-0" />
                ) : (
                  <ServerIcon size={12} className="text-slate-600 shrink-0" />
                )}
                <span className="truncate">{machineLabel}</span>
              </span>
            )}
          </div>
        </div>
      </div>

      {/* Actions — reveal on hover, but always render so layout doesn't shift */}
      <div className="server-card-actions">
        {server.status === 'stopped' ? (
          <button
            onClick={handleStart}
            disabled={isLoading}
            className="btn btn-success btn-sm"
          >
            <Play size={12} strokeWidth={2.5} />
            Start
          </button>
        ) : server.status === 'running' ? (
          <button
            onClick={handleStop}
            disabled={isLoading}
            className="btn btn-secondary btn-sm"
          >
            <Square size={12} strokeWidth={2.5} />
            Stop
          </button>
        ) : null}

        <button
          onClick={handleDelete}
          disabled={isLoading || server.status === 'running'}
          className="btn btn-danger btn-sm"
          aria-label="Delete server"
        >
          <Trash2 size={12} strokeWidth={2.2} />
        </button>

        <span className="ml-auto inline-flex items-center gap-1 text-[11.5px] font-medium text-slate-500 group-hover:text-orange-300 transition-colors">
          Open
          <ArrowRight size={11} />
        </span>
      </div>
    </div>
  );
}
