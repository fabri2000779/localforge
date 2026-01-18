import { useEffect, useState, useRef } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { 
  ArrowLeft, Play, Square, Trash2, RefreshCw, Send, Folder, 
  Cpu, HardDrive, Terminal, Settings, RotateCcw, Copy, 
  Clock, Network, FolderOpen, Check, Save, Globe, Wifi, ExternalLink, Key
} from 'lucide-react';
import { useServerStore } from '../stores/serverStore';
import { useGamesStore } from '../stores/gamesStore';
import { findGameConfig } from '../utils/gameTypes';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-shell';
import { ConsoleOutput } from '../components/ConsoleOutput';
import { DeleteConfirmDialog } from '../components/DeleteConfirmDialog';
import { GameIcon } from '../components/GameIcon';
import { FileManager } from '../components/FileManager';

type TabType = 'console' | 'files' | 'network' | 'settings';

export function ServerDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  
  const servers = useServerStore((s) => s.servers);
  const isLoading = useServerStore((s) => s.isLoading);
  const stats = useServerStore((s) => s.stats);
  const fetchServers = useServerStore((s) => s.fetchServers);
  const startStatsPolling = useServerStore((s) => s.startStatsPolling);
  const stopStatsPolling = useServerStore((s) => s.stopStatsPolling);
  const startServer = useServerStore((s) => s.startServer);
  const stopServer = useServerStore((s) => s.stopServer);
  const deleteServer = useServerStore((s) => s.deleteServer);
  const sendCommand = useServerStore((s) => s.sendCommand);
  const updateServerConfig = useServerStore((s) => s.updateServerConfig);
  const reinstallServer = useServerStore((s) => s.reinstallServer);
  const updateServerGame = useServerStore((s) => s.updateServerGame);
  
  const { games } = useGamesStore();

  const [activeTab, setActiveTab] = useState<TabType>('console');
  const [command, setCommand] = useState('');
  const [logs, setLogs] = useState<string[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);
  const userScrolledRef = useRef(false);
  const lastLogCountRef = useRef(0);
  const [commandHistory, setCommandHistory] = useState<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState(-1);
  const [uptime, setUptime] = useState<string>('');
  const [copied, setCopied] = useState<string | null>(null);
  const [editingConfig, setEditingConfig] = useState<Record<string, string>>({});
  const [isEditingConfig, setIsEditingConfig] = useState(false);
  const [showDeleteDialog, setShowDeleteDialog] = useState(false);
  const [configSaved, setConfigSaved] = useState(false);
  const [publicIP, setPublicIP] = useState<string>('');
  const [oauthUrl, setOauthUrl] = useState<string | null>(null);
  const [oauthCode, setOauthCode] = useState<string | null>(null);
  const [oauthDismissed, setOauthDismissed] = useState(false);
  const [diskUsage, setDiskUsage] = useState<number>(0);
  
  const consoleRef = useRef<HTMLDivElement>(null);
  const consoleEndRef = useRef<HTMLDivElement>(null);
  const unlistenRef = useRef<(() => void) | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const server = servers.find((s) => s.id === id);
  const gameConfig = server ? findGameConfig(games, server.game_type) : null;

  // Fetch public IP
  useEffect(() => {
    fetch('https://api.ipify.org?format=json')
      .then(res => res.json())
      .then(data => setPublicIP(data.ip))
      .catch(() => setPublicIP(''));
  }, []);

  // Fetch disk usage
  useEffect(() => {
    if (!id) return;
    
    const fetchDiskUsage = async () => {
      try {
        const size = await invoke<number>('get_server_disk_usage', { serverId: id });
        setDiskUsage(size);
      } catch (e) {
        console.error('Failed to fetch disk usage:', e);
      }
    };
    
    fetchDiskUsage();
    // Refresh disk usage every 30 seconds
    const interval = setInterval(fetchDiskUsage, 30000);
    return () => clearInterval(interval);
  }, [id]);

  // Main initialization effect
  useEffect(() => {
    if (!id) return;
    
    fetchServers();
    
    const fetchInitialLogs = async () => {
      try {
        const response = await invoke<{ logs: string[] }>('get_server_logs', { serverId: id, lines: 500 });
        setLogs(response.logs);
      } catch (e) {
        console.error('[ServerDetail] Failed to fetch logs:', e);
      }
    };
    
    const setupStreaming = async () => {
      try {
        const unlisten = await listen<{ server_id: string; line: string }>('server-log', (event) => {
          if (event.payload.server_id === id) {
            setLogs((prev) => [...prev, event.payload.line]);
          }
        });
        
        unlistenRef.current = unlisten;
        await invoke('attach_server', { serverId: id });
        setIsStreaming(true);
      } catch (e) {
        console.error('[ServerDetail] Streaming setup failed:', e);
      }
    };
    
    fetchInitialLogs();
    setupStreaming();
    startStatsPolling(id);
    
    return () => {
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
      invoke('detach_server', { serverId: id }).catch(() => {});
      stopStatsPolling();
    };
  }, [id]);

  // Initialize editing config when server loads
  useEffect(() => {
    if (server?.config) {
      setEditingConfig(server.config);
    }
  }, [server?.config]);

  // Uptime calculator
  useEffect(() => {
    if (server?.status !== 'running') {
      setUptime('');
      return;
    }
    
    const startTime = new Date(server.created_at);
    const updateUptime = () => {
      const now = new Date();
      const diff = now.getTime() - startTime.getTime();
      const hours = Math.floor(diff / (1000 * 60 * 60));
      const minutes = Math.floor((diff % (1000 * 60 * 60)) / (1000 * 60));
      const seconds = Math.floor((diff % (1000 * 60)) / 1000);
      
      if (hours > 0) {
        setUptime(`${hours}h ${minutes}m`);
      } else if (minutes > 0) {
        setUptime(`${minutes}m ${seconds}s`);
      } else {
        setUptime(`${seconds}s`);
      }
    };
    
    updateUptime();
    const interval = setInterval(updateUptime, 1000);
    return () => clearInterval(interval);
  }, [server?.status, server?.created_at]);

  // Clear logs when server stops
  const prevStatusRef = useRef(server?.status);
  useEffect(() => {
    if (prevStatusRef.current === 'running' && server?.status === 'stopped') {
      // Server just stopped - clear logs
      setLogs([]);
      setIsStreaming(false);
      userScrolledRef.current = false;
      setAutoScroll(true);
    }
    prevStatusRef.current = server?.status;
  }, [server?.status]);

  // Auto-scroll - use instant for fast updates
  useEffect(() => {
    if (autoScroll && consoleEndRef.current && !userScrolledRef.current) {
      // Use instant scroll for fast log updates
      consoleEndRef.current.scrollIntoView({ behavior: 'instant' });
    }
    lastLogCountRef.current = logs.length;
  }, [logs.length, autoScroll]);

  // Detect OAuth URLs in logs (for Hytale and similar games)
  useEffect(() => {
    // Don't detect if server is stopped or user dismissed or we already have a URL
    if (server?.status === 'stopped' || server?.status === 'error') {
      if (oauthUrl) {
        setOauthUrl(null);
        setOauthCode(null);
      }
      // Reset dismissed flag when server stops so it can show again on next start
      setOauthDismissed(false);
      return;
    }
    
    // Don't re-detect if user dismissed or already detected
    if (oauthDismissed || oauthUrl) return;
    
    // Search from newest to oldest log lines
    const recentLogs = logs.slice(-30).reverse();
    for (const line of recentLogs) {
      // Strip ANSI codes and any special characters
      const cleanLine = line
        .replace(/\x1b\[[0-9;]*m/g, '')
        .replace(/\u001b\[[0-9;]*m/g, '')
        .replace(/[\x00-\x1F]/g, ''); // Remove control characters
      
      // Look for the specific Hytale OAuth URL with user_code parameter
      // The URL format is: https://oauth.accounts.hytale.com/oauth2/device/verify?user_code=XXXXX
      const urlRegex = /https:\/\/oauth\.accounts\.hytale\.com\/oauth2\/device\/verify\?user_code=([A-Za-z0-9]+)/;
      const match = cleanLine.match(urlRegex);
      
      if (match) {
        const detectedUrl = match[0];
        const detectedCode = match[1];
        setOauthUrl(detectedUrl);
        setOauthCode(detectedCode);
        return;
      }
    }
  }, [logs, oauthUrl, oauthDismissed, server?.status]);

  const handleScroll = () => {
    if (consoleRef.current) {
      const { scrollTop, scrollHeight, clientHeight } = consoleRef.current;
      const isAtBottom = scrollHeight - scrollTop - clientHeight < 50;
      
      // If user scrolled up, mark as user-scrolled
      if (!isAtBottom && lastLogCountRef.current === logs.length) {
        userScrolledRef.current = true;
        setAutoScroll(false);
      }
      
      // If user scrolled back to bottom, re-enable auto-scroll
      if (isAtBottom) {
        userScrolledRef.current = false;
        setAutoScroll(true);
      }
    }
  };

  // Open folder in file explorer
  const openFolder = async (path: string) => {
    try {
      await open(path);
    } catch (e) {
      console.error('Failed to open folder:', e);
    }
  };

  // Copy to clipboard with feedback
  const copyToClipboard = async (text: string, key: string) => {
    await navigator.clipboard.writeText(text);
    setCopied(key);
    setTimeout(() => setCopied(null), 2000);
  };

  if (!server) {
    return (
      <div className="animate-fade-in">
        <button onClick={() => navigate('/servers')} className="flex items-center gap-2 text-zinc-400 hover:text-white mb-6">
          <ArrowLeft size={20} /> Back to Servers
        </button>
        <div className="card text-center py-12">
          <div className="text-5xl mb-4">🔍</div>
          <h2 className="text-xl font-semibold mb-2">Server not found</h2>
          <p className="text-zinc-400">Loading...</p>
        </div>
      </div>
    );
  }

  const handleSendCommand = async () => {
    if (!command.trim()) return;
    const cmd = command.trim();
    setCommandHistory(prev => [...prev.filter(c => c !== cmd), cmd]);
    setHistoryIndex(-1);
    setLogs(prev => [...prev, `> ${cmd}`]);
    setCommand('');
    await sendCommand(server.id, cmd);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      handleSendCommand();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (commandHistory.length > 0) {
        const newIndex = historyIndex < commandHistory.length - 1 ? historyIndex + 1 : historyIndex;
        setHistoryIndex(newIndex);
        setCommand(commandHistory[commandHistory.length - 1 - newIndex] || '');
      }
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (historyIndex > 0) {
        const newIndex = historyIndex - 1;
        setHistoryIndex(newIndex);
        setCommand(commandHistory[commandHistory.length - 1 - newIndex] || '');
      } else {
        setHistoryIndex(-1);
        setCommand('');
      }
    }
  };

  const handleStart = async () => {
    setLogs([]);
    userScrolledRef.current = false;
    setAutoScroll(true);
    setOauthUrl(null);
    setOauthCode(null);
    setOauthDismissed(false); // Reset so OAuth can be detected again
    await startServer(server.id);
  };

  const handleStop = async () => {
    setLogs(['Stopping server...']);
    setOauthUrl(null);
    setOauthCode(null);
    await stopServer(server.id);
  };

  const handleRestart = async () => {
    setLogs(['Restarting server...']);
    await stopServer(server.id);
    setTimeout(async () => {
      await startServer(server.id);
    }, 2000);
  };

  const handleDelete = async (deleteData: boolean) => {
    setShowDeleteDialog(false);
    await deleteServer(server.id, deleteData);
    navigate('/servers');
  };

  const handleRefresh = async () => {
    try {
      const response = await invoke<{ logs: string[] }>('get_server_logs', { serverId: server.id, lines: 500 });
      setLogs(response.logs);
      await invoke('attach_server', { serverId: server.id });
    } catch (e) {
      console.error('Refresh failed:', e);
    }
  };

  const handleClearLogs = () => {
    setLogs([]);
    userScrolledRef.current = false;
    setAutoScroll(true);
    setOauthUrl(null);
    setOauthCode(null);
    setOauthDismissed(false); // Reset so OAuth can be detected again
  };
  
  const handleSaveConfig = async () => {
    const success = await updateServerConfig(server.id, editingConfig);
    if (success) {
      setIsEditingConfig(false);
      setConfigSaved(true);
      setTimeout(() => setConfigSaved(false), 3000);
    }
  };

  // Get friendly label for config key
  const getConfigLabel = (key: string): string => {
    const labels: Record<string, string> = {
      'TYPE': 'Server Type',
      'VERSION': 'Game Version',
      'MEMORY': 'Memory Allocation',
      'MAX_PLAYERS': 'Max Players',
      'DIFFICULTY': 'Difficulty',
      'MODE': 'Game Mode',
      'MOTD': 'Server Message (MOTD)',
      'EULA': 'EULA Accepted',
      'ENABLE_RCON': 'RCON Enabled',
      'RCON_PASSWORD': 'RCON Password',
      'CREATE_CONSOLE_IN_PIPE': 'Console Pipe',
      'SERVER_NAME': 'Server Name',
      'WORLD_NAME': 'World Name',
      'SERVER_PASS': 'Password',
      'SERVER_PUBLIC': 'Public Server',
      'GAMEMODE': 'Game Mode',
      'ONLINE_MODE': 'Online Mode',
      'SPAWN_PROTECTION': 'Spawn Protection',
      'VIEW_DISTANCE': 'View Distance',
      'LEVEL_TYPE': 'Level Type',
      'SEED': 'World Seed',
    };
    return labels[key] || key.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase());
  };

  // Check if a config value should be hidden
  const isSecretConfig = (key: string): boolean => {
    return key.toLowerCase().includes('password') || key.toLowerCase().includes('secret');
  };

  // Check if a config value is internal (shouldn't be edited)
  const isInternalConfig = (key: string): boolean => {
    return ['EULA', 'CREATE_CONSOLE_IN_PIPE', 'ENABLE_RCON'].includes(key);
  };

  const statusColors = {
    running: { bg: 'bg-green-500/10', border: 'border-green-500/30', text: 'text-green-500', dot: 'bg-green-500' },
    stopped: { bg: 'bg-zinc-500/10', border: 'border-zinc-500/30', text: 'text-zinc-400', dot: 'bg-zinc-500' },
    starting: { bg: 'bg-yellow-500/10', border: 'border-yellow-500/30', text: 'text-yellow-500', dot: 'bg-yellow-500' },
    installing: { bg: 'bg-blue-500/10', border: 'border-blue-500/30', text: 'text-blue-500', dot: 'bg-blue-500' },
    stopping: { bg: 'bg-yellow-500/10', border: 'border-yellow-500/30', text: 'text-yellow-500', dot: 'bg-yellow-500' },
    error: { bg: 'bg-red-500/10', border: 'border-red-500/30', text: 'text-red-500', dot: 'bg-red-500' },
  };

  // Format bytes to human readable
  const formatBytes = (bytes: number): string => {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  };

  const status = statusColors[server.status] || statusColors.stopped;
  const serverAddress = `localhost:${server.port}`;
  const publicAddress = publicIP ? `${publicIP}:${server.port}` : '';

  return (
    <div className="animate-fade-in">
      {/* Header with Back Button and Address */}
      <div className="flex items-center justify-between mb-6">
        <button onClick={() => navigate('/servers')} className="flex items-center gap-2 text-zinc-400 hover:text-white transition-colors">
          <ArrowLeft size={20} /> Back to Servers
        </button>
        
        {/* Connection Address */}
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2 px-4 py-2 bg-zinc-800 rounded-lg border border-zinc-700">
            <Network size={16} className="text-cyan-400" />
            <span className="text-sm text-zinc-400">Connect:</span>
            <code className="font-mono font-semibold text-white">{serverAddress}</code>
            <button 
              onClick={() => copyToClipboard(serverAddress, 'local')}
              className="ml-2 p-1 hover:bg-zinc-700 rounded transition-colors"
              title="Copy address"
            >
              {copied === 'local' ? <Check size={14} className="text-green-500" /> : <Copy size={14} className="text-zinc-400" />}
            </button>
          </div>
        </div>
      </div>

      {/* Server Info Card */}
      <div className="card mb-6">
        <div className="flex items-start gap-6">
          {/* Icon & Name */}
          <div className="flex items-center gap-4 flex-1">
            <GameIcon 
              icon={gameConfig?.icon || '🎮'} 
              logoUrl={gameConfig?.logo_url}
              name={gameConfig?.name || server.game_type}
              size="xl"
            />
            <div>
              <h1 className="text-2xl font-bold">{server.name}</h1>
              <p className="text-zinc-400 text-sm">{gameConfig?.name || server.game_type}</p>
            </div>
          </div>
          
          {/* Status Badge */}
          <div className={`px-4 py-2 rounded-lg ${status.bg} ${status.border} border flex items-center gap-2`}>
            <span className={`w-2 h-2 rounded-full ${status.dot} ${server.status === 'running' || server.status === 'starting' ? 'animate-pulse' : ''}`}></span>
            <span className={`font-medium capitalize ${status.text}`}>{server.status}</span>
          </div>
        </div>

        {/* Stats Bar - Only when running */}
        {server.status === 'running' && stats && (
          <div className="mt-6 p-4 bg-zinc-800/50 rounded-lg">
            <div className="flex items-center gap-8">
              {/* CPU */}
              <div className="flex items-center gap-3">
                <div className="w-10 h-10 rounded-lg bg-indigo-500/20 flex items-center justify-center">
                  <Cpu size={20} className="text-indigo-400" />
                </div>
                <div>
                  <div className="text-xs text-zinc-500 uppercase tracking-wide">CPU</div>
                  <div className="text-lg font-semibold">{stats.cpu_percent.toFixed(1)}%</div>
                </div>
              </div>
              
              {/* Memory */}
              <div className="flex items-center gap-3 flex-1">
                <div className="w-10 h-10 rounded-lg bg-green-500/20 flex items-center justify-center">
                  <HardDrive size={20} className="text-green-400" />
                </div>
                <div className="flex-1">
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-zinc-500 uppercase tracking-wide">Memory</span>
                    <span className="text-xs text-zinc-400">
                      {stats.memory_usage_mb.toFixed(0)} / {stats.memory_limit_mb.toFixed(0)} MB
                    </span>
                  </div>
                  <div className="mt-1 h-2 bg-zinc-700 rounded-full overflow-hidden">
                    <div 
                      className={`h-full rounded-full transition-all duration-300 ${
                        stats.memory_percent > 90 ? 'bg-red-500' : 
                        stats.memory_percent > 70 ? 'bg-yellow-500' : 'bg-green-500'
                      }`}
                      style={{ width: `${Math.min(stats.memory_percent, 100)}%` }} 
                    />
                  </div>
                </div>
                <div className="text-lg font-semibold w-16 text-right">{stats.memory_percent.toFixed(0)}%</div>
              </div>
              
              {/* Disk - always visible */}
              <div className="flex items-center gap-3">
                <div className="w-10 h-10 rounded-lg bg-cyan-500/20 flex items-center justify-center">
                  <Folder size={20} className="text-cyan-400" />
                </div>
                <div>
                  <div className="text-xs text-zinc-500 uppercase tracking-wide">Disk</div>
                  <div className="text-lg font-semibold">{formatBytes(diskUsage)}</div>
                </div>
              </div>
              
              {/* Uptime */}
              {uptime && (
                <div className="flex items-center gap-3">
                  <div className="w-10 h-10 rounded-lg bg-purple-500/20 flex items-center justify-center">
                    <Clock size={20} className="text-purple-400" />
                  </div>
                  <div>
                    <div className="text-xs text-zinc-500 uppercase tracking-wide">Uptime</div>
                    <div className="text-lg font-semibold">{uptime}</div>
                  </div>
                </div>
              )}
            </div>
          </div>
        )}

        {/* Disk Usage - Always visible when server is NOT running */}
        {server.status !== 'running' && (
          <div className="mt-6 p-4 bg-zinc-800/50 rounded-lg">
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-lg bg-cyan-500/20 flex items-center justify-center">
                <Folder size={20} className="text-cyan-400" />
              </div>
              <div>
                <div className="text-xs text-zinc-500 uppercase tracking-wide">Disk Usage</div>
                <div className="text-lg font-semibold">{formatBytes(diskUsage)}</div>
              </div>
            </div>
          </div>
        )}

        {/* Action Buttons */}
        <div className="flex items-center gap-3 mt-6 pt-6 border-t border-zinc-800">
          {server.status === 'stopped' ? (
            <button onClick={handleStart} disabled={isLoading} className="btn btn-success">
              <Play size={18} /> Start Server
            </button>
          ) : server.status === 'running' ? (
            <>
              <button onClick={handleStop} disabled={isLoading} className="btn btn-secondary">
                <Square size={18} /> Stop
              </button>
              <button onClick={handleRestart} disabled={isLoading} className="btn btn-secondary">
                <RotateCcw size={18} /> Restart
              </button>
            </>
          ) : (
            <button disabled className="btn btn-secondary">
              <RefreshCw size={18} className="animate-spin" /> {server.status}...
            </button>
          )}
          
          <button 
            onClick={() => openFolder(String(server.data_path))} 
            className="btn btn-secondary"
          >
            <FolderOpen size={18} /> Open Folder
          </button>
          
          <div className="flex-1" />
          
          <button 
            onClick={() => setShowDeleteDialog(true)} 
            disabled={isLoading || server.status === 'running'} 
            className="btn btn-danger"
          >
            <Trash2 size={18} /> Delete
          </button>
        </div>
      </div>

      {/* Delete Confirmation Dialog */}
      <DeleteConfirmDialog
        isOpen={showDeleteDialog}
        serverName={server.name}
        dataPath={String(server.data_path)}
        onConfirm={handleDelete}
        onCancel={() => setShowDeleteDialog(false)}
      />

      {/* OAuth Authentication Popup (for Hytale etc.) */}
      {oauthUrl && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={() => { setOauthUrl(null); setOauthDismissed(true); }} />
          <div className="relative bg-zinc-900 border border-indigo-500/50 rounded-xl shadow-2xl max-w-md w-full mx-4 p-6 animate-fade-in">
            <div className="flex items-center gap-3 mb-4">
              <div className="w-12 h-12 rounded-lg bg-indigo-500/20 flex items-center justify-center">
                <Key size={24} className="text-indigo-400" />
              </div>
              <div>
                <h3 className="text-lg font-semibold">Authentication Required</h3>
                <p className="text-sm text-zinc-400">Hytale needs you to log in</p>
              </div>
            </div>
            
            <p className="text-zinc-300 mb-4">
              This server requires authentication with your Hytale account. Click the button below to open the authentication page.
            </p>
            
            {oauthCode && (
              <div className="mb-4 p-3 bg-zinc-800 rounded-lg">
                <div className="text-xs text-zinc-500 mb-1">Authorization Code</div>
                <div className="flex items-center justify-between">
                  <code className="text-2xl font-mono font-bold text-white tracking-wider">{oauthCode}</code>
                  <button 
                    onClick={() => {
                      navigator.clipboard.writeText(oauthCode);
                      setCopied('oauth');
                      setTimeout(() => setCopied(null), 2000);
                    }}
                    className="p-2 hover:bg-zinc-700 rounded transition-colors"
                  >
                    {copied === 'oauth' ? <Check size={16} className="text-green-500" /> : <Copy size={16} className="text-zinc-400" />}
                  </button>
                </div>
              </div>
            )}
            
            <div className="flex items-center gap-3">
              <button
                onClick={() => oauthUrl && open(oauthUrl)}
                className="btn btn-primary flex-1"
              >
                <ExternalLink size={16} /> Open Authentication Page
              </button>
              <button
                onClick={() => { setOauthUrl(null); setOauthDismissed(true); }}
                className="btn btn-secondary"
              >
                Dismiss
              </button>
            </div>
            
            <p className="text-xs text-zinc-500 mt-4 text-center">
              After authenticating, the server will continue starting automatically.
            </p>
          </div>
        </div>
      )}

      {/* Tabs */}
      <div className="flex items-center gap-1 mb-4">
        <button
          onClick={() => setActiveTab('console')}
          className={`px-4 py-2 rounded-lg font-medium transition-colors flex items-center gap-2 ${
            activeTab === 'console' 
              ? 'bg-zinc-800 text-white' 
              : 'text-zinc-400 hover:text-white hover:bg-zinc-800/50'
          }`}
        >
          <Terminal size={18} /> Console
        </button>
        <button
          onClick={() => setActiveTab('files')}
          className={`px-4 py-2 rounded-lg font-medium transition-colors flex items-center gap-2 ${
            activeTab === 'files' 
              ? 'bg-zinc-800 text-white' 
              : 'text-zinc-400 hover:text-white hover:bg-zinc-800/50'
          }`}
        >
          <Folder size={18} /> Files
        </button>
        <button
          onClick={() => setActiveTab('network')}
          className={`px-4 py-2 rounded-lg font-medium transition-colors flex items-center gap-2 ${
            activeTab === 'network' 
              ? 'bg-zinc-800 text-white' 
              : 'text-zinc-400 hover:text-white hover:bg-zinc-800/50'
          }`}
        >
          <Globe size={18} /> Network
        </button>
        <button
          onClick={() => setActiveTab('settings')}
          className={`px-4 py-2 rounded-lg font-medium transition-colors flex items-center gap-2 ${
            activeTab === 'settings' 
              ? 'bg-zinc-800 text-white' 
              : 'text-zinc-400 hover:text-white hover:bg-zinc-800/50'
          }`}
        >
          <Settings size={18} /> Settings
        </button>
      </div>

      {/* Tab Content */}
      {activeTab === 'console' && (
        <div className="card p-0 overflow-hidden">
          <div className="console-toolbar">
            <Terminal size={16} className="text-zinc-400 mr-2" />
            <span className="text-sm font-medium text-zinc-300">Console</span>
            
            {server.status === 'running' && (
              <span className={`ml-2 text-xs flex items-center gap-1 ${isStreaming ? 'text-green-500' : 'text-yellow-500'}`}>
                <span className={`w-1.5 h-1.5 rounded-full ${isStreaming ? 'bg-green-500 animate-pulse' : 'bg-yellow-500'}`}></span>
                {isStreaming ? 'Live' : 'Connecting...'}
              </span>
            )}
            
            <div className="flex-1" />
            
            <span className="text-xs text-zinc-500 mr-3">{logs.length} lines</span>
            
            <button onClick={handleClearLogs} className="console-toolbar-btn">Clear</button>
            <button onClick={handleRefresh} className="console-toolbar-btn">Refresh</button>
            
            {!autoScroll && (
              <button 
                onClick={() => {
                  userScrolledRef.current = false;
                  setAutoScroll(true);
                  consoleEndRef.current?.scrollIntoView({ behavior: 'instant' });
                }}
                className="console-toolbar-btn text-indigo-400"
              >
                ↓ Bottom
              </button>
            )}
          </div>

          <ConsoleOutput
            logs={logs}
            consoleRef={consoleRef}
            consoleEndRef={consoleEndRef}
            onScroll={handleScroll}
            emptyMessage={
              server.status === 'running' 
                ? 'Waiting for logs...' 
                : server.status === 'stopping'
                  ? 'Stopping server...'
                  : 'Server is stopped. Press Start to begin.'
            }
          />

          {server.status === 'running' && gameConfig?.console && (
            <div className="console-input-wrapper">
              <span className="text-green-500">❯</span>
              <input
                ref={inputRef}
                type="text"
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                onKeyDown={handleKeyDown}
                className="console-input"
                placeholder="Type a command... (↑↓ for history)"
              />
              <button 
                onClick={handleSendCommand} 
                disabled={!command.trim()} 
                className="text-zinc-500 hover:text-white disabled:opacity-30 transition-colors"
              >
                <Send size={16} />
              </button>
            </div>
          )}
          
          {server.status === 'running' && !gameConfig?.console && (
            <div className="px-4 py-3 border-t border-zinc-800 text-sm text-zinc-500 flex items-center gap-2">
              <Terminal size={16} />
              Console commands not supported for this game
            </div>
          )}
        </div>
      )}

      {activeTab === 'files' && (
        <div className="h-[600px]">
          <FileManager 
            rootPath={String(server.data_path)} 
            serverName={server.name}
          />
        </div>
      )}

      {activeTab === 'network' && (
        <div className="card">
          <h3 className="text-lg font-semibold mb-4 flex items-center gap-2">
            <Globe size={20} /> Network & Public Access
          </h3>
          
          <div className="space-y-4">
            {/* Local Address */}
            <div className="flex items-center justify-between p-4 bg-zinc-800/50 rounded-lg">
              <div className="flex items-center gap-3">
                <div className="w-10 h-10 rounded-lg bg-cyan-500/20 flex items-center justify-center">
                  <Wifi size={20} className="text-cyan-400" />
                </div>
                <div>
                  <div className="font-medium">Local Network</div>
                  <div className="text-sm text-zinc-400">Players on your network can join</div>
                </div>
              </div>
              <div className="flex items-center gap-2">
                <code className="px-3 py-1.5 bg-zinc-900 rounded font-mono">{serverAddress}</code>
                <button 
                  onClick={() => copyToClipboard(serverAddress, 'local2')}
                  className="p-2 hover:bg-zinc-700 rounded transition-colors"
                >
                  {copied === 'local2' ? <Check size={16} className="text-green-500" /> : <Copy size={16} className="text-zinc-400" />}
                </button>
              </div>
            </div>
            
            {/* Public Address */}
            <div className="p-4 bg-zinc-800/50 rounded-lg border border-zinc-700">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div className="w-10 h-10 rounded-lg bg-green-500/20 flex items-center justify-center">
                    <Globe size={20} className="text-green-400" />
                  </div>
                  <div>
                    <div className="font-medium">Public Internet</div>
                    <div className="text-sm text-zinc-400">Allow anyone to join from the internet</div>
                  </div>
                </div>
              </div>
              
              {publicIP && (
                <div className="mt-4 p-3 bg-zinc-900 rounded-lg">
                  <div className="flex items-center justify-between mb-2">
                    <span className="text-sm text-zinc-400">Your Public Address</span>
                    <button 
                      onClick={() => copyToClipboard(publicAddress, 'public')}
                      className="text-xs text-indigo-400 hover:text-indigo-300 flex items-center gap-1"
                    >
                      {copied === 'public' ? <Check size={12} /> : <Copy size={12} />}
                      Copy
                    </button>
                  </div>
                  <code className="text-lg font-mono text-white">{publicAddress}</code>
                </div>
              )}
              
              <div className="mt-4 p-3 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
                <div className="flex items-start gap-2 text-sm text-yellow-400">
                  <span className="text-yellow-500 mt-0.5">⚠️</span>
                  <div>
                    <div className="font-medium mb-1">Port Forwarding Required</div>
                    <p className="text-yellow-400/80">
                      To make your server public, you need to forward port <strong>{server.port}</strong> (TCP/UDP) in your router settings.
                    </p>
                    <button 
                      onClick={() => open('https://portforward.com/')}
                      className="mt-2 text-yellow-300 hover:text-yellow-200 flex items-center gap-1"
                    >
                      Learn how to port forward <ExternalLink size={12} />
                    </button>
                  </div>
                </div>
              </div>
              
              <div className="mt-4 p-4 bg-gradient-to-r from-indigo-500/10 to-purple-500/10 border border-indigo-500/30 rounded-lg">
                <div className="flex items-start gap-3">
                  <div className="w-10 h-10 rounded-lg bg-indigo-500/20 flex items-center justify-center flex-shrink-0">
                    <span className="text-xl">🌊</span>
                  </div>
                  <div className="flex-1">
                    <div className="font-semibold text-white mb-1">Want hassle-free public hosting?</div>
                    <p className="text-sm text-zinc-400 mb-3">
                      Skip port forwarding and router configs. Serverwave provides instant public game servers with DDoS protection, automatic backups, and 24/7 uptime.
                    </p>
                    <div className="flex items-center gap-3">
                      <button 
                        onClick={() => open('https://serverwave.com/')}
                        className="btn btn-primary text-sm"
                      >
                        Try Serverwave <ExternalLink size={14} />
                      </button>
                      <span className="text-xs text-zinc-500">Free tier available</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      )}

      {activeTab === 'settings' && (
        <div className="space-y-6">
          {/* Server Configuration */}
          <div className="card">
            <div className="flex items-center justify-between mb-6">
              <h3 className="text-lg font-semibold">Game Configuration</h3>
              {isEditingConfig ? (
                <div className="flex items-center gap-2">
                  <button 
                    onClick={() => {
                      setIsEditingConfig(false);
                      setEditingConfig(server.config);
                    }} 
                    className="btn btn-secondary"
                  >
                    Cancel
                  </button>
                  <button onClick={handleSaveConfig} className="btn btn-success">
                    <Save size={16} /> Save Changes
                  </button>
                </div>
              ) : (
                <button 
                  onClick={() => setIsEditingConfig(true)} 
                  className="btn btn-secondary"
                >
                  <Settings size={16} /> Edit Configuration
                </button>
              )}
            </div>
            
            {configSaved && (
              <div className="mb-4 p-3 bg-green-500/10 border border-green-500/30 rounded-lg text-sm text-green-400 flex items-center gap-2">
                <Check size={16} /> Configuration saved! Changes will apply on next restart.
              </div>
            )}
            
            {isEditingConfig && (
              <div className="mb-4 p-3 bg-blue-500/10 border border-blue-500/30 rounded-lg text-sm text-blue-400">
                💡 Changes will be applied on next server restart
              </div>
            )}
            
            <div className="space-y-4">
              {/* Server Info */}
              <div className="grid grid-cols-2 md:grid-cols-4 gap-4 pb-4 border-b border-zinc-800">
                <div>
                  <label className="text-xs text-zinc-500 uppercase tracking-wide">Server ID</label>
                  <div className="mt-1 font-mono text-sm">{server.id}</div>
                </div>
                <div>
                  <label className="text-xs text-zinc-500 uppercase tracking-wide">Port</label>
                  <div className="mt-1 font-mono text-sm">{server.port}</div>
                </div>
                <div>
                  <label className="text-xs text-zinc-500 uppercase tracking-wide">Game</label>
                  <div className="mt-1 text-sm">{gameConfig?.name || server.game_type}</div>
                </div>
                <div>
                  <label className="text-xs text-zinc-500 uppercase tracking-wide">Created</label>
                  <div className="mt-1 text-sm">{new Date(server.created_at).toLocaleDateString()}</div>
                </div>
              </div>
              
              {/* Configuration Options */}
              <div>
                <h4 className="font-medium mb-4">Game Settings</h4>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  {Object.entries(editingConfig)
                    .filter(([key]) => !isInternalConfig(key))
                    .map(([key, value]) => (
                    <div key={key} className="space-y-1">
                      <label className="text-sm text-zinc-400">{getConfigLabel(key)}</label>
                      {isEditingConfig ? (
                        <input
                          type={isSecretConfig(key) ? 'password' : 'text'}
                          value={editingConfig[key] || ''}
                          onChange={(e) => setEditingConfig(prev => ({ ...prev, [key]: e.target.value }))}
                          className="input"
                        />
                      ) : (
                        <div className="bg-zinc-800 rounded-lg px-3 py-2 font-mono text-sm">
                          {isSecretConfig(key) ? '••••••••' : value || '-'}
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </div>
          
          {/* Maintenance */}
          {gameConfig?.install_script && (
            <div className="card">
              <h3 className="text-lg font-semibold mb-4 flex items-center gap-2">
                <RefreshCw size={20} /> Maintenance
              </h3>
              
              <div className="space-y-4">
                {/* Update Button */}
                <div className="flex items-center justify-between p-4 bg-zinc-800/50 rounded-lg">
                  <div>
                    <div className="font-medium">Update Server</div>
                    <div className="text-sm text-zinc-400">Run install script again to update game files (keeps your data)</div>
                  </div>
                  <button 
                    onClick={async () => {
                      setLogs([]);
                      await updateServerGame(server.id);
                    }}
                    disabled={isLoading || server.status === 'running' || server.status === 'installing'}
                    className="btn btn-secondary"
                  >
                    <RefreshCw size={16} /> Update
                  </button>
                </div>
                
                {/* Reinstall Button */}
                <div className="flex items-center justify-between p-4 bg-yellow-500/5 border border-yellow-500/20 rounded-lg">
                  <div>
                    <div className="font-medium text-yellow-400">Reinstall Server</div>
                    <div className="text-sm text-zinc-400">Delete all data and reinstall from scratch</div>
                  </div>
                  <button 
                    onClick={async () => {
                      if (confirm('Are you sure you want to reinstall? This will DELETE ALL server data including worlds and configs!')) {
                        setLogs([]);
                        await reinstallServer(server.id);
                      }
                    }}
                    disabled={isLoading || server.status === 'running' || server.status === 'installing'}
                    className="btn btn-secondary border-yellow-500/50 text-yellow-400 hover:bg-yellow-500/10"
                  >
                    <RotateCcw size={16} /> Reinstall
                  </button>
                </div>
              </div>
            </div>
          )}
          
          {/* Danger Zone */}
          <div className="card border-red-900/30">
            <h4 className="font-medium text-red-400 mb-3">Danger Zone</h4>
            <div className="flex items-center justify-between p-4 bg-red-950/20 border border-red-900/50 rounded-lg">
              <div>
                <div className="font-medium">Delete Server</div>
                <div className="text-sm text-zinc-400">Remove the server and optionally delete all data.</div>
              </div>
              <button 
                onClick={() => setShowDeleteDialog(true)} 
                disabled={isLoading || server.status === 'running'} 
                className="btn btn-danger"
              >
                <Trash2 size={18} /> Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
