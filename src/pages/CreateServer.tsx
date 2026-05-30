import { useState, useEffect } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';
import { ArrowLeft, Loader2, AlertTriangle, Server } from 'lucide-react';
import { useServerStore } from '../stores/serverStore';
import { useGamesStore } from '../stores/gamesStore';
import { useNodesStore } from '../stores/nodesStore';
import { GameIcon } from '../components/GameIcon';
import type { GameType, CreateServerRequest, Variable } from '../types';

const RAM_OPTIONS = [
  { value: 1024, label: '1 GB' },
  { value: 2048, label: '2 GB' },
  { value: 3072, label: '3 GB' },
  { value: 4096, label: '4 GB' },
  { value: 6144, label: '6 GB' },
  { value: 8192, label: '8 GB' },
  { value: 10240, label: '10 GB' },
  { value: 12288, label: '12 GB' },
  { value: 16384, label: '16 GB' },
];

export function CreateServer() {
  const navigate = useNavigate();
  const location = useLocation();
  const { createServer, isLoading, error, clearError } = useServerStore();
  const { games } = useGamesStore();
  const { nodes, activeNodeId } = useNodesStore();
  // The server is created on whichever node is active in the top-left
  // switcher. Surface it so the user knows where it'll land — and how to
  // change it — before they commit.
  const activeNode = nodes.find((n) => n.id === activeNodeId);

  const initialGameType = (location.state as { gameType?: GameType })?.gameType;

  const [selectedGame, setSelectedGame] = useState<GameType | null>(initialGameType || null);
  const [serverName, setServerName] = useState('');
  const [port, setPort] = useState<number | undefined>(undefined);
  const [memoryMb, setMemoryMb] = useState<number>(2048);
  const [config, setConfig] = useState<Record<string, string>>({});

  const gameConfig = selectedGame ? games.find((g) => g.game_type === selectedGame) : null;

  useEffect(() => {
    if (gameConfig) {
      const defaults: Record<string, string> = {};
      gameConfig.variables
        .filter((v) => v.user_editable && !v.system_mapping || v.system_mapping === 'none')
        .forEach((v) => {
          defaults[v.env] = v.default;
        });
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setConfig(defaults);

      const defaultPort = gameConfig.ports[0]?.container_port || 25565;
      setPort(defaultPort);

      const recommended = gameConfig.recommended_ram_mb;
      const closest = RAM_OPTIONS.reduce((prev, curr) =>
        Math.abs(curr.value - recommended) < Math.abs(prev.value - recommended) ? curr : prev
      );
      setMemoryMb(closest.value);
    }
  }, [gameConfig]);

  useEffect(() => {
    return () => clearError();
  }, [clearError]);

  const handleCreate = async () => {
    if (!selectedGame || !serverName.trim()) return;

    const request: CreateServerRequest = {
      name: serverName.trim(),
      game_type: selectedGame,
      port,
      config,
      memory_mb: memoryMb,
    };

    const server = await createServer(request);
    if (server) {
      navigate(`/servers/${server.id}`);
    }
  };

  const renderVariable = (variable: Variable) => {
    const value = config[variable.env] || variable.default;

    if (variable.options && variable.options.length > 0) {
      return (
        <select
          value={value}
          onChange={(e) => setConfig({ ...config, [variable.env]: e.target.value })}
          className="input"
        >
          {variable.options.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
      );
    }

    switch (variable.field_type) {
      case 'number':
        return (
          <input
            type="number"
            value={value}
            onChange={(e) => setConfig({ ...config, [variable.env]: e.target.value })}
            className="input"
          />
        );
      case 'password':
        return (
          <input
            type="password"
            value={value}
            onChange={(e) => setConfig({ ...config, [variable.env]: e.target.value })}
            className="input"
            placeholder={variable.default}
          />
        );
      case 'text':
      default:
        return (
          <input
            type="text"
            value={value}
            onChange={(e) => setConfig({ ...config, [variable.env]: e.target.value })}
            className="input"
            placeholder={variable.default}
          />
        );
    }
  };

  const editableVariables = gameConfig?.variables.filter((v) => 
    v.user_editable && (!v.system_mapping || v.system_mapping === 'none')
  ) || [];

  return (
    <div className="animate-fade-in max-w-3xl">
      <button
        onClick={() => navigate(-1)}
        className="inline-flex items-center gap-1.5 text-[12.5px] font-medium text-slate-400 hover:text-white mb-5 transition-colors"
      >
        <ArrowLeft size={14} strokeWidth={2.2} />
        Back
      </button>

      <header className="page-header">
        <div className="eyebrow mb-2">New server</div>
        <h1 className="page-title">Create Server</h1>
        <p className="page-subtitle">
          Choose a game and configure your server in a few steps.
        </p>
      </header>

      <div
        className="flex items-start gap-2 text-[12.5px] text-slate-400 mb-6 px-3 py-2.5 rounded-lg"
        style={{
          background: 'rgba(249,115,22,0.06)',
          border: '1px solid rgba(249,115,22,0.15)',
        }}
      >
        <Server size={14} className="text-orange-300 shrink-0 mt-0.5" />
        <span>
          This server will be created on{' '}
          <span className="font-medium text-slate-200">
            {activeNode?.label ?? 'this machine'}
          </span>
          . To use a different machine or VPS, switch nodes in the selector at
          the top-left first.
        </span>
      </div>

      {!selectedGame ? (
        <section>
          <div className="section-header">
            <div className="section-title">Select a game</div>
            <span className="text-[11.5px] text-slate-500">
              {games.length} templates
            </span>
          </div>
          <div className="grid grid-cols-2 md:grid-cols-3 gap-3">
            {games.map((game) => (
              <button
                key={game.game_type}
                onClick={() => setSelectedGame(game.game_type)}
                className="game-tile group"
              >
                <GameIcon
                  icon={game.icon}
                  logoUrl={game.logo_url}
                  name={game.name}
                  size="lg"
                  className="mb-3"
                />
                <h3 className="font-semibold text-[13.5px] text-slate-100">
                  {game.name}
                </h3>
                <p className="text-[11.5px] text-slate-500 mt-1.5 line-clamp-2 leading-snug">
                  {game.description}
                </p>
                <div className="mt-3 flex items-center gap-2 text-[10.5px] uppercase tracking-wider font-semibold text-slate-500">
                  <span>Min {game.min_ram_mb} MB</span>
                </div>
              </button>
            ))}
          </div>
        </section>
      ) : (
        <section>
          <div className="card mb-6 flex items-center gap-4">
            <GameIcon 
              icon={gameConfig?.icon || '🎮'} 
              logoUrl={gameConfig?.logo_url} 
              name={gameConfig?.name || ''}
              size="lg"
            />
            <div className="flex-1">
              <h2 className="text-xl font-semibold">{gameConfig?.name}</h2>
              <p className="text-sm text-slate-400">{gameConfig?.description}</p>
            </div>
            <button onClick={() => setSelectedGame(null)} className="btn btn-secondary text-sm">
              Change
            </button>
          </div>

          <div className="card mb-6">
            <h3 className="text-lg font-semibold mb-4">Basic Settings</h3>

            <div className="space-y-4">
              <div>
                <label className="input-label">Server Name *</label>
                <input
                  type="text"
                  value={serverName}
                  onChange={(e) => setServerName(e.target.value)}
                  className="input"
                  placeholder="My Awesome Server"
                />
              </div>

              <div>
                <label className="input-label">Port</label>
                <input
                  type="number"
                  value={port || ''}
                  onChange={(e) => setPort(parseInt(e.target.value) || undefined)}
                  className="input"
                  placeholder={gameConfig?.ports[0]?.container_port.toString()}
                />
                <p className="text-xs text-slate-500 mt-1">
                  Default: {gameConfig?.ports[0]?.container_port || 25565}
                </p>
              </div>

              <div>
                <label className="input-label">Memory (RAM)</label>
                <select
                  value={memoryMb}
                  onChange={(e) => setMemoryMb(parseInt(e.target.value))}
                  className="input"
                >
                  {RAM_OPTIONS.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
                {gameConfig && memoryMb < gameConfig.min_ram_mb && (
                  <div className="flex items-center gap-2 mt-2 text-yellow-400 text-sm">
                    <AlertTriangle size={16} />
                    <span>Below minimum ({gameConfig.min_ram_mb} MB) - server may not start</span>
                  </div>
                )}
                <p className="text-xs text-slate-500 mt-1">
                  Recommended: {gameConfig?.recommended_ram_mb ? `${Math.round(gameConfig.recommended_ram_mb / 1024)} GB` : '2 GB'}
                  {gameConfig?.min_ram_mb && ` • Minimum: ${gameConfig.min_ram_mb} MB`}
                </p>
              </div>
            </div>
          </div>

          {editableVariables.length > 0 && (
            <div className="card mb-6">
              <h3 className="text-lg font-semibold mb-4">Game Settings</h3>
              <div className="space-y-4">
                {editableVariables.map((variable) => (
                  <div key={variable.env}>
                    <label className="input-label">{variable.name}</label>
                    {renderVariable(variable)}
                    {variable.description && (
                      <p className="text-xs text-slate-500 mt-1">{variable.description}</p>
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}

          {error && (
            <div className="card bg-red-900/20 border-red-800 mb-6">
              <p className="text-red-400">{error}</p>
            </div>
          )}

          <button
            onClick={handleCreate}
            disabled={isLoading || !serverName.trim()}
            className="btn btn-primary w-full justify-center py-3"
          >
            {isLoading ? (
              <>
                <Loader2 size={20} className="animate-spin" />
                Creating Server...
              </>
            ) : (
              'Create Server'
            )}
          </button>
        </section>
      )}
    </div>
  );
}
