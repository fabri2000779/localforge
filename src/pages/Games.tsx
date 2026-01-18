import { useEffect, useState } from 'react';
import { 
  Plus, Download, Upload, Trash2, Edit2, Save, X, 
  Package, Settings, ChevronDown, ChevronUp, Copy, Check,
  RotateCcw, ExternalLink, FolderOpen, FileCode, ArrowRight
} from 'lucide-react';
import { useGamesStore } from '../stores/gamesStore';
import { GameConfig, DEFAULT_GAME_CONFIG, Variable, PortConfig, ConfigFile, SystemMapping, FieldType, ConfigFileFormat } from '../types';
import { open } from '@tauri-apps/plugin-shell';
import { GameIcon } from '../components/GameIcon';

type EditMode = 'none' | 'create' | 'edit';

export function GamesPage() {
  const { games, error, configPath, fetchGames, fetchConfigPath, addGame, deleteGame, 
          exportGame, exportAllCustomGames, importGame, resetToDefaults, clearError } = useGamesStore();
  
  const [editMode, setEditMode] = useState<EditMode>('none');
  const [editingGame, setEditingGame] = useState<GameConfig | null>(null);
  const [originalGameType, setOriginalGameType] = useState<string>('');
  const [expandedGame, setExpandedGame] = useState<string | null>(null);
  const [importJson, setImportJson] = useState('');
  const [showImportDialog, setShowImportDialog] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  
  // New variable form
  const [newVarEnv, setNewVarEnv] = useState('');
  const [newVarName, setNewVarName] = useState('');
  const [newVarDefault, setNewVarDefault] = useState('');
  const [newVarMapping, setNewVarMapping] = useState<SystemMapping>('none');
  const [newVarEditable, setNewVarEditable] = useState(true);
  
  // New port form
  const [newPortContainer, setNewPortContainer] = useState('');
  const [newPortProtocol, setNewPortProtocol] = useState<'tcp' | 'udp' | 'both'>('tcp');
  const [newPortDescription, setNewPortDescription] = useState('');
  const [newPortEnvVar, setNewPortEnvVar] = useState('');
  
  // New config file form
  const [newCfgPath, setNewCfgPath] = useState('');
  const [newCfgFormat, setNewCfgFormat] = useState<ConfigFileFormat>('json');
  const [cfgVarInputs, setCfgVarInputs] = useState<Record<number, { key: string; value: string }>>({});

  useEffect(() => {
    fetchGames();
    fetchConfigPath();
  }, []);

  const handleStartCreate = () => {
    setEditingGame({ ...DEFAULT_GAME_CONFIG, game_type: `custom-${Date.now()}` });
    setOriginalGameType('');
    setEditMode('create');
  };

  const handleStartEdit = (game: GameConfig) => {
    setEditingGame({ ...game, is_custom: true });
    setOriginalGameType(game.game_type);
    setEditMode('edit');
  };

  const handleSave = async () => {
    if (!editingGame) return;
    if (!editingGame.game_type || !editingGame.name || !editingGame.docker_image) {
      alert('Please fill in Game ID, Name, and Docker Image');
      return;
    }
    const success = await addGame(editingGame);
    if (success) {
      setEditMode('none');
      setEditingGame(null);
      setOriginalGameType('');
    }
  };

  const handleCancel = () => {
    setEditMode('none');
    setEditingGame(null);
    setOriginalGameType('');
  };

  const handleDelete = async (gameType: string) => {
    const game = games.find(g => g.game_type === gameType);
    if (!game?.is_custom) return;
    if (confirm('Delete this custom game definition?')) {
      await deleteGame(gameType);
    }
  };

  const handleExport = async (gameType: string) => {
    const json = await exportGame(gameType);
    if (json) {
      await navigator.clipboard.writeText(json);
      setCopied(gameType);
      setTimeout(() => setCopied(null), 2000);
    }
  };

  const handleExportAll = async () => {
    const json = await exportAllCustomGames();
    if (json) {
      await navigator.clipboard.writeText(json);
      setCopied('all');
      setTimeout(() => setCopied(null), 2000);
    }
  };

  const handleImport = async () => {
    if (!importJson.trim()) return;
    const game = await importGame(importJson);
    if (game) {
      setShowImportDialog(false);
      setImportJson('');
    }
  };

  const handleReset = async () => {
    if (confirm('Reset all games to defaults?')) {
      await resetToDefaults();
    }
  };

  const handleOpenConfigFolder = async () => {
    if (configPath) {
      try { await open(configPath); } catch (e) { console.error('Failed to open folder:', e); }
    }
  };

  const updateEditingGame = (updates: Partial<GameConfig>) => {
    if (editingGame) setEditingGame({ ...editingGame, ...updates });
  };

  // Variable management
  const addVariable = () => {
    if (!newVarEnv.trim() || !editingGame) return;
    const newVar: Variable = {
      env: newVarEnv, name: newVarName || newVarEnv, description: '', default: newVarDefault,
      system_mapping: newVarMapping, user_editable: newVarEditable, field_type: 'text' as FieldType,
    };
    updateEditingGame({ variables: [...editingGame.variables, newVar] });
    setNewVarEnv(''); setNewVarName(''); setNewVarDefault(''); setNewVarMapping('none'); setNewVarEditable(true);
  };

  const updateVariable = (index: number, updates: Partial<Variable>) => {
    if (!editingGame) return;
    const newVars = [...editingGame.variables];
    newVars[index] = { ...newVars[index], ...updates };
    updateEditingGame({ variables: newVars });
  };

  const removeVariable = (index: number) => {
    if (!editingGame) return;
    const newVars = [...editingGame.variables];
    newVars.splice(index, 1);
    updateEditingGame({ variables: newVars });
  };

  // Port management
  const addPort = () => {
    if (!newPortContainer.trim() || !editingGame) return;
    const portNum = parseInt(newPortContainer);
    if (isNaN(portNum) || portNum < 1 || portNum > 65535) { alert('Invalid port number'); return; }
    const newPort: PortConfig = { 
      container_port: portNum, 
      protocol: newPortProtocol, 
      description: newPortDescription || undefined,
      env_var: newPortEnvVar || undefined
    };
    updateEditingGame({ ports: [...editingGame.ports, newPort] });
    setNewPortContainer(''); setNewPortProtocol('tcp'); setNewPortDescription(''); setNewPortEnvVar('');
  };

  const updatePort = (index: number, updates: Partial<PortConfig>) => {
    if (!editingGame) return;
    const newPorts = [...editingGame.ports];
    newPorts[index] = { ...newPorts[index], ...updates };
    updateEditingGame({ ports: newPorts });
  };

  const removePort = (index: number) => {
    if (!editingGame) return;
    const newPorts = [...editingGame.ports];
    newPorts.splice(index, 1);
    updateEditingGame({ ports: newPorts });
  };

  // Config file management
  const addConfigFile = () => {
    if (!newCfgPath.trim() || !editingGame) return;
    const newCfg: ConfigFile = { path: newCfgPath, format: newCfgFormat, variables: {} };
    updateEditingGame({ config_files: [...editingGame.config_files, newCfg] });
    setNewCfgPath(''); setNewCfgFormat('json');
  };

  const updateConfigFile = (index: number, updates: Partial<ConfigFile>) => {
    if (!editingGame) return;
    const newFiles = [...editingGame.config_files];
    newFiles[index] = { ...newFiles[index], ...updates };
    updateEditingGame({ config_files: newFiles });
  };

  const removeConfigFile = (index: number) => {
    if (!editingGame) return;
    const newFiles = [...editingGame.config_files];
    newFiles.splice(index, 1);
    updateEditingGame({ config_files: newFiles });
    const newInputs = { ...cfgVarInputs };
    delete newInputs[index];
    setCfgVarInputs(newInputs);
  };

  const updateCfgVarInput = (index: number, field: 'key' | 'value', val: string) => {
    setCfgVarInputs(prev => ({
      ...prev,
      [index]: { ...prev[index], key: prev[index]?.key || '', value: prev[index]?.value || '', [field]: val }
    }));
  };

  const addConfigVar = (cfgIndex: number) => {
    const input = cfgVarInputs[cfgIndex];
    if (!input?.key?.trim() || !input?.value?.trim() || !editingGame) return;
    const newFiles = [...editingGame.config_files];
    newFiles[cfgIndex] = { ...newFiles[cfgIndex], variables: { ...newFiles[cfgIndex].variables, [input.key]: input.value } };
    updateEditingGame({ config_files: newFiles });
    setCfgVarInputs(prev => ({ ...prev, [cfgIndex]: { key: '', value: '' } }));
  };

  const updateConfigVar = (cfgIndex: number, oldKey: string, newKey: string, newValue: string) => {
    if (!editingGame) return;
    const newFiles = [...editingGame.config_files];
    const newVars = { ...newFiles[cfgIndex].variables };
    // If key changed, delete old key
    if (oldKey !== newKey) {
      delete newVars[oldKey];
    }
    newVars[newKey] = newValue;
    newFiles[cfgIndex] = { ...newFiles[cfgIndex], variables: newVars };
    updateEditingGame({ config_files: newFiles });
  };

  const removeConfigVar = (cfgIndex: number, key: string) => {
    if (!editingGame) return;
    const newFiles = [...editingGame.config_files];
    const newVars = { ...newFiles[cfgIndex].variables };
    delete newVars[key];
    newFiles[cfgIndex] = { ...newFiles[cfgIndex], variables: newVars };
    updateEditingGame({ config_files: newFiles });
  };

  // Get variables that can be mapped to ports (port-type or number-type with PORT in name)
  const getPortMappedVariables = () => {
    if (!editingGame) return [];
    return editingGame.variables.filter(v => 
      v.system_mapping === 'port' || 
      (v.field_type === 'number' && v.env.toUpperCase().includes('PORT'))
    );
  };

  const customGames = games.filter(g => g.is_custom);
  const builtinGames = games.filter(g => !g.is_custom);

  return (
    <div className="animate-fade-in max-w-6xl mx-auto">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold">Game Definitions</h1>
          <p className="text-slate-400 mt-1">Manage game server templates</p>
        </div>
        <div className="flex items-center gap-2">
          <button onClick={handleOpenConfigFolder} className="btn btn-secondary" title="Open config folder">
            <FolderOpen size={18} />
          </button>
          <button onClick={() => setShowImportDialog(true)} className="btn btn-secondary">
            <Upload size={18} /> Import
          </button>
          {customGames.length > 0 && (
            <button onClick={handleExportAll} className="btn btn-secondary">
              {copied === 'all' ? <Check size={18} className="text-emerald-500" /> : <Download size={18} />}
              Export Custom
            </button>
          )}
          <button onClick={handleStartCreate} className="btn btn-primary">
            <Plus size={18} /> New Game
          </button>
        </div>
      </div>

      {error && (
        <div className="mb-4 p-3 bg-red-500/10 border border-red-500/30 rounded-lg text-red-400 flex items-center justify-between">
          <span>{error}</span>
          <button onClick={clearError} className="text-red-400 hover:text-red-300"><X size={18} /></button>
        </div>
      )}

      {configPath && (
        <div className="mb-4 p-3 bg-slate-800/50 rounded-lg flex items-center justify-between text-sm">
          <div className="flex items-center gap-2 text-slate-400">
            <FolderOpen size={16} />
            <span>Config:</span>
            <code className="text-slate-300">{configPath}</code>
          </div>
          <button onClick={handleOpenConfigFolder} className="text-blue-400 hover:text-blue-300 flex items-center gap-1">
            Open <ExternalLink size={12} />
          </button>
        </div>
      )}

      {/* Edit/Create Form */}
      {editMode !== 'none' && editingGame && (
        <div className="card mb-6 border-2 border-blue-500/50">
          <div className="flex items-center justify-between mb-4">
            <h3 className="text-lg font-semibold">
              {editMode === 'create' ? 'Create New Game' : `Edit: ${editingGame.name}`}
            </h3>
            <div className="flex items-center gap-2">
              <button onClick={handleCancel} className="btn btn-secondary"><X size={16} /> Cancel</button>
              <button onClick={handleSave} className="btn btn-success"><Save size={16} /> Save</button>
            </div>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
            {/* Basic Info */}
            <div className="space-y-4">
              <div>
                <label className="block text-sm text-slate-400 mb-1">Game ID *</label>
                <input type="text" value={editingGame.game_type}
                  onChange={(e) => updateEditingGame({ game_type: e.target.value.toLowerCase().replace(/\s+/g, '-') })}
                  className="input" placeholder="my-custom-game" disabled={editMode === 'edit'} />
              </div>
              <div>
                <label className="block text-sm text-slate-400 mb-1">Name *</label>
                <input type="text" value={editingGame.name}
                  onChange={(e) => updateEditingGame({ name: e.target.value })}
                  className="input" placeholder="My Custom Game" />
              </div>
              <div>
                <label className="block text-sm text-slate-400 mb-1">Logo URL</label>
                <div className="flex gap-3 items-stretch">
                  <input type="text" value={editingGame.logo_url || ''}
                    onChange={(e) => updateEditingGame({ logo_url: e.target.value || undefined })}
                    className="flex-1 min-w-0 px-4 py-2 bg-slate-700 border border-slate-600 rounded-lg text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500" 
                    placeholder="https://example.com/logo.png" />
                  <div className="w-[42px] bg-slate-700 rounded-lg flex items-center justify-center flex-shrink-0 overflow-hidden">
                    <GameIcon icon={editingGame.icon} logoUrl={editingGame.logo_url} name={editingGame.name} size="md" />
                  </div>
                </div>
              </div>
              <div>
                <label className="block text-sm text-slate-400 mb-1">Description</label>
                <textarea value={editingGame.description}
                  onChange={(e) => updateEditingGame({ description: e.target.value })}
                  className="input min-h-[80px]" placeholder="Description..." />
              </div>
            </div>

            {/* Docker Config */}
            <div className="space-y-4">
              <div>
                <label className="block text-sm text-slate-400 mb-1">Docker Image *</label>
                <input type="text" value={editingGame.docker_image}
                  onChange={(e) => updateEditingGame({ docker_image: e.target.value })}
                  className="input" placeholder="username/image:tag" />
              </div>
              <div>
                <label className="block text-sm text-slate-400 mb-1">Startup Command</label>
                <input type="text" value={editingGame.startup}
                  onChange={(e) => updateEditingGame({ startup: e.target.value })}
                  className="input font-mono text-sm" placeholder="java -Xmx{{SERVER_MEMORY}}M -jar server.jar" />
                <p className="text-xs text-slate-500 mt-1">Use {'{{VAR}}'} for placeholders</p>
              </div>
              <div>
                <label className="block text-sm text-slate-400 mb-1">Stop Command</label>
                <input type="text" value={editingGame.stop_command}
                  onChange={(e) => updateEditingGame({ stop_command: e.target.value })}
                  className="input" placeholder="stop" />
              </div>
              <div>
                <label className="block text-sm text-slate-400 mb-1">Volume Path</label>
                <input type="text" value={editingGame.volume_path}
                  onChange={(e) => updateEditingGame({ volume_path: e.target.value })}
                  className="input" placeholder="/data" />
              </div>
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <label className="block text-sm text-slate-400 mb-1">Min RAM (MB)</label>
                  <input type="number" value={editingGame.min_ram_mb}
                    onChange={(e) => updateEditingGame({ min_ram_mb: parseInt(e.target.value) || 512 })}
                    className="input" />
                </div>
                <div>
                  <label className="block text-sm text-slate-400 mb-1">Recommended RAM (MB)</label>
                  <input type="number" value={editingGame.recommended_ram_mb}
                    onChange={(e) => updateEditingGame({ recommended_ram_mb: parseInt(e.target.value) || 2048 })}
                    className="input" />
                </div>
              </div>
            </div>
          </div>

          {/* Install Script */}
          <div className="mt-6 pt-6 border-t border-slate-700">
            <h4 className="font-medium mb-3">Install Script (optional)</h4>
            <p className="text-xs text-slate-500 mb-3">Shell script to run on first start</p>
            <div className="mb-4">
              <label className="block text-sm text-slate-400 mb-1">Install Docker Image</label>
              <input type="text" value={editingGame.install_image || ''}
                onChange={(e) => updateEditingGame({ install_image: e.target.value || undefined })}
                className="input" placeholder="alpine:latest (leave empty to use main Docker image)" />
              <p className="text-xs text-slate-500 mt-1">Docker image used to run the install script. Leave empty to use the main Docker image.</p>
            </div>
            <div>
              <label className="block text-sm text-slate-400 mb-1">Install Script</label>
              <textarea value={editingGame.install_script || ''}
                onChange={(e) => updateEditingGame({ install_script: e.target.value || undefined })}
                className="input font-mono text-sm min-h-[100px]" placeholder="#!/bin/bash&#10;curl -o server.jar https://..." />
            </div>
          </div>

          {/* Variables */}
          <div className="mt-6 pt-6 border-t border-slate-700">
            <h4 className="font-medium mb-3">Environment Variables</h4>
            
            <div className="max-w-3xl">
              {/* Header row */}
              <div className="grid grid-cols-[120px_100px_1fr_80px_32px] gap-2 text-xs text-slate-500 mb-2 px-2">
                <span>ENV Key</span>
                <span>Display Name</span>
                <span>Default Value</span>
                <span>Mapping</span>
                <span></span>
              </div>
              
              <div className="space-y-2 mb-4">
                {editingGame.variables.map((v, i) => (
                  <div key={i} className="grid grid-cols-[120px_100px_1fr_80px_32px] gap-2 items-center bg-slate-800/50 p-2 rounded-lg">
                    <input type="text" value={v.env} onChange={(e) => updateVariable(i, { env: e.target.value.toUpperCase() })}
                      className="input font-mono text-xs py-1.5" placeholder="ENV_KEY" />
                    <input type="text" value={v.name} onChange={(e) => updateVariable(i, { name: e.target.value })}
                      className="input text-xs py-1.5" placeholder="Name" />
                    <input type="text" value={v.default} onChange={(e) => updateVariable(i, { default: e.target.value })}
                      className="input text-xs py-1.5" placeholder="Default value" />
                    <select value={v.system_mapping || 'none'} onChange={(e) => updateVariable(i, { system_mapping: e.target.value as SystemMapping })}
                      className="input text-xs py-1.5">
                      <option value="none">-</option>
                      <option value="ram">RAM</option>
                      <option value="port">Port</option>
                    </select>
                    <button onClick={() => removeVariable(i)} className="p-1.5 text-red-400 hover:bg-red-500/20 rounded justify-self-center">
                      <X size={14} />
                    </button>
                  </div>
                ))}
              </div>
              
              {/* Add new variable row */}
              <div className="grid grid-cols-[120px_100px_1fr_80px_auto] gap-2 items-center">
                <input type="text" value={newVarEnv} onChange={(e) => setNewVarEnv(e.target.value.toUpperCase())}
                  className="input font-mono" placeholder="ENV_KEY" />
                <input type="text" value={newVarName} onChange={(e) => setNewVarName(e.target.value)}
                  className="input" placeholder="Name" />
                <input type="text" value={newVarDefault} onChange={(e) => setNewVarDefault(e.target.value)}
                  className="input" placeholder="Default value" />
                <div></div>
                <button onClick={addVariable} disabled={!newVarEnv.trim()} className="btn btn-secondary">
                  <Plus size={16} /> Add
                </button>
              </div>
            </div>
          </div>

          {/* Ports - Now Editable */}
          <div className="mt-6 pt-6 border-t border-slate-700">
            <h4 className="font-medium mb-3">Ports</h4>
            <p className="text-xs text-slate-500 mb-3">
              Assign an ENV variable to each port so different ports can be configured independently.
            </p>
            
            <div className="max-w-2xl">
              {/* Header row */}
              <div className="grid grid-cols-[80px_80px_1fr_120px_32px] gap-2 text-xs text-slate-500 mb-2 px-2">
                <span>Port</span>
                <span>Protocol</span>
                <span>Description</span>
                <span>ENV Variable</span>
                <span></span>
              </div>
              
              <div className="space-y-2 mb-4">
                {editingGame.ports.map((p, i) => (
                  <div key={i} className="grid grid-cols-[80px_80px_1fr_120px_32px] gap-2 items-center bg-slate-800/50 p-2 rounded-lg">
                    <input type="number" value={p.container_port} 
                      onChange={(e) => updatePort(i, { container_port: parseInt(e.target.value) || 0 })}
                      className="input font-mono text-xs py-1.5" placeholder="25565" />
                    <select value={p.protocol} onChange={(e) => updatePort(i, { protocol: e.target.value as 'tcp' | 'udp' | 'both' })}
                      className="input text-xs py-1.5">
                      <option value="tcp">TCP</option>
                      <option value="udp">UDP</option>
                      <option value="both">Both</option>
                    </select>
                    <input type="text" value={p.description || ''} 
                      onChange={(e) => updatePort(i, { description: e.target.value || undefined })}
                      className="input text-xs py-1.5" placeholder="Game port" />
                    <select value={p.env_var || ''} 
                      onChange={(e) => updatePort(i, { env_var: e.target.value || undefined })}
                      className="input text-xs py-1.5">
                      <option value="">- Select ENV -</option>
                      {getPortMappedVariables().map(v => (
                        <option key={v.env} value={v.env}>{v.env}</option>
                      ))}
                    </select>
                    <button onClick={() => removePort(i)} className="p-1.5 text-red-400 hover:bg-red-500/20 rounded justify-self-center">
                      <X size={14} />
                    </button>
                  </div>
                ))}
              </div>
              
              {/* Add new port row */}
              <div className="grid grid-cols-[80px_80px_1fr_120px_auto] gap-2 items-center">
                <input type="number" value={newPortContainer} onChange={(e) => setNewPortContainer(e.target.value)}
                  className="input font-mono" placeholder="25565" />
                <select value={newPortProtocol} onChange={(e) => setNewPortProtocol(e.target.value as 'tcp' | 'udp' | 'both')}
                  className="input">
                  <option value="tcp">TCP</option>
                  <option value="udp">UDP</option>
                  <option value="both">Both</option>
                </select>
                <input type="text" value={newPortDescription} onChange={(e) => setNewPortDescription(e.target.value)}
                  className="input" placeholder="Description" />
                <select value={newPortEnvVar} onChange={(e) => setNewPortEnvVar(e.target.value)}
                  className="input">
                  <option value="">- Select ENV -</option>
                  {getPortMappedVariables().map(v => (
                    <option key={v.env} value={v.env}>{v.env}</option>
                  ))}
                </select>
                <button onClick={addPort} disabled={!newPortContainer.trim()} className="btn btn-secondary">
                  <Plus size={16} /> Add
                </button>
              </div>
            </div>
          </div>

          {/* Config Files - Fully Editable */}
          <div className="mt-6 pt-6 border-t border-slate-700">
            <div className="flex items-center gap-2 mb-2">
              <FileCode size={18} className="text-cyan-400" />
              <h4 className="font-medium">Config Files</h4>
            </div>
            <p className="text-xs text-slate-500 mb-4">
              Map environment variables to config file keys. Values like <code className="text-cyan-400">{'{{VAR}}'}</code> will be replaced at runtime.
            </p>
            
            {/* Existing config files */}
            <div className="space-y-4 mb-4">
              {editingGame.config_files.map((cfg, cfgIndex) => (
                <div key={cfgIndex} className="bg-slate-800/60 rounded-xl overflow-hidden">
                  {/* File header - Now Editable */}
                  <div className="flex items-center gap-3 px-4 py-3 bg-slate-700/50">
                    <FileCode size={16} className="text-cyan-400 flex-shrink-0" />
                    <input type="text" value={cfg.path}
                      onChange={(e) => updateConfigFile(cfgIndex, { path: e.target.value })}
                      className="flex-1 px-3 py-1.5 bg-slate-900 border border-slate-600 rounded-lg text-cyan-300 text-sm font-mono placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-cyan-500" 
                      placeholder="config.json" />
                    <select value={cfg.format} 
                      onChange={(e) => updateConfigFile(cfgIndex, { format: e.target.value as ConfigFileFormat })}
                      className="input w-32 text-sm">
                      <option value="json">JSON</option>
                      <option value="yaml">YAML</option>
                      <option value="properties">Properties</option>
                    </select>
                    <button onClick={() => removeConfigFile(cfgIndex)} 
                      className="p-1.5 text-slate-400 hover:text-red-400 hover:bg-red-500/10 rounded-lg transition-colors">
                      <Trash2 size={14} />
                    </button>
                  </div>
                  
                  {/* Variable mappings - Now Editable */}
                  <div className="p-4">
                    {Object.entries(cfg.variables).length > 0 ? (
                      <div className="space-y-2 mb-4">
                        <div className="text-xs text-slate-500 mb-2">Variable mappings:</div>
                        {Object.entries(cfg.variables).map(([key, value]) => (
                          <ConfigVarRow 
                            key={key}
                            configKey={key}
                            configValue={value}
                            onUpdate={(newKey, newValue) => updateConfigVar(cfgIndex, key, newKey, newValue)}
                            onRemove={() => removeConfigVar(cfgIndex, key)}
                          />
                        ))}
                      </div>
                    ) : (
                      <div className="text-sm text-slate-500 italic mb-4 py-2">No mappings defined</div>
                    )}
                    
                    {/* Add new mapping */}
                    <div className="flex items-center gap-2 pt-3 border-t border-slate-700/50">
                      <input type="text" value={cfgVarInputs[cfgIndex]?.key || ''}
                        onChange={(e) => updateCfgVarInput(cfgIndex, 'key', e.target.value)}
                        className="input flex-1 text-sm" placeholder="Config key (e.g. MaxPlayers)" />
                      <ArrowRight size={14} className="text-slate-500 flex-shrink-0" />
                      <input type="text" value={cfgVarInputs[cfgIndex]?.value || ''}
                        onChange={(e) => updateCfgVarInput(cfgIndex, 'value', e.target.value)}
                        className="input flex-1 text-sm font-mono" placeholder="{{ENV_VAR}}" />
                      <button onClick={() => addConfigVar(cfgIndex)} 
                        disabled={!cfgVarInputs[cfgIndex]?.key?.trim() || !cfgVarInputs[cfgIndex]?.value?.trim()}
                        className="btn btn-secondary text-sm py-2 px-3">
                        <Plus size={14} />
                      </button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
            
            {/* Add new config file */}
            <div className="flex items-center gap-3 p-4 bg-slate-800/30 rounded-xl border-2 border-dashed border-slate-700 hover:border-slate-600 transition-colors">
              <FileCode size={18} className="text-slate-500" />
              <input type="text" value={newCfgPath} onChange={(e) => setNewCfgPath(e.target.value)}
                className="input flex-1" placeholder="config.json" />
              <select value={newCfgFormat} onChange={(e) => setNewCfgFormat(e.target.value as ConfigFileFormat)}
                className="input w-32">
                <option value="json">JSON</option>
                <option value="yaml">YAML</option>
                <option value="properties">Properties</option>
              </select>
              <button onClick={addConfigFile} disabled={!newCfgPath.trim()} className="btn btn-primary">
                <Plus size={16} /> Add File
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Custom Games */}
      {customGames.length > 0 && (
        <div className="mb-8">
          <h2 className="text-lg font-semibold mb-4 flex items-center gap-2">
            <Package size={20} className="text-blue-400" />
            Custom Games ({customGames.length})
          </h2>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {customGames.map(game => (
              <GameCard key={game.game_type} game={game} expanded={expandedGame === game.game_type}
                onToggle={() => setExpandedGame(expandedGame === game.game_type ? null : game.game_type)}
                onEdit={() => handleStartEdit(game)} onDelete={() => handleDelete(game.game_type)}
                onExport={() => handleExport(game.game_type)} copied={copied === game.game_type} />
            ))}
          </div>
        </div>
      )}

      {/* Built-in Games */}
      <div>
        <h2 className="text-lg font-semibold mb-4 flex items-center gap-2">
          <Settings size={20} className="text-emerald-400" />
          Built-in Games ({builtinGames.length})
        </h2>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {builtinGames.map(game => (
            <GameCard key={game.game_type} game={game} expanded={expandedGame === game.game_type}
              onToggle={() => setExpandedGame(expandedGame === game.game_type ? null : game.game_type)}
              onEdit={() => handleStartEdit(game)} onExport={() => handleExport(game.game_type)} copied={copied === game.game_type} />
          ))}
        </div>
      </div>

      {/* Reset */}
      <div className="mt-8 pt-6 border-t border-slate-700">
        <button onClick={handleReset} className="btn btn-secondary text-slate-400">
          <RotateCcw size={16} /> Reset to Defaults
        </button>
      </div>

      {/* Import Dialog */}
      {showImportDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={() => setShowImportDialog(false)} />
          <div className="relative bg-slate-900 border border-slate-700 rounded-xl shadow-2xl max-w-lg w-full mx-4 p-6">
            <h3 className="text-lg font-semibold mb-4">Import Game Definition</h3>
            <textarea value={importJson} onChange={(e) => setImportJson(e.target.value)}
              className="input min-h-[200px] font-mono text-sm" placeholder="Paste game JSON here..." />
            <div className="flex items-center justify-end gap-2 mt-4">
              <button onClick={() => setShowImportDialog(false)} className="btn btn-secondary">Cancel</button>
              <button onClick={handleImport} disabled={!importJson.trim()} className="btn btn-primary">
                <Upload size={16} /> Import
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// Editable config variable row component
interface ConfigVarRowProps {
  configKey: string;
  configValue: string;
  onUpdate: (newKey: string, newValue: string) => void;
  onRemove: () => void;
}

function ConfigVarRow({ configKey, configValue, onUpdate, onRemove }: ConfigVarRowProps) {
  const [key, setKey] = useState(configKey);
  const [value, setValue] = useState(configValue);

  // Update parent when blur
  const handleKeyBlur = () => {
    if (key !== configKey || value !== configValue) {
      onUpdate(key, value);
    }
  };

  const handleValueBlur = () => {
    if (key !== configKey || value !== configValue) {
      onUpdate(key, value);
    }
  };

  return (
    <div className="flex items-center gap-2 group">
      <input type="text" value={key}
        onChange={(e) => setKey(e.target.value)}
        onBlur={handleKeyBlur}
        className="input flex-1 text-sm bg-slate-900/50" />
      <ArrowRight size={14} className="text-slate-500 flex-shrink-0" />
      <input type="text" value={value}
        onChange={(e) => setValue(e.target.value)}
        onBlur={handleValueBlur}
        className="input flex-1 text-sm font-mono text-cyan-400 bg-slate-900/50" />
      <button onClick={onRemove} 
        className="p-1.5 text-slate-500 hover:text-red-400 hover:bg-red-500/10 rounded-lg opacity-0 group-hover:opacity-100 transition-all">
        <X size={14} />
      </button>
    </div>
  );
}

interface GameCardProps {
  game: GameConfig;
  expanded: boolean;
  onToggle: () => void;
  onEdit: () => void;
  onDelete?: () => void;
  onExport: () => void;
  copied: boolean;
}

function GameCard({ game, expanded, onToggle, onEdit, onDelete, onExport, copied }: GameCardProps) {
  const defaultPort = game.ports[0]?.container_port || 25565;
  
  return (
    <div className={`card ${game.is_custom ? 'border border-blue-500/30' : ''}`}>
      <div className="flex items-start gap-4">
        <GameIcon icon={game.icon} logoUrl={game.logo_url} name={game.name} size="lg" />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="font-semibold">{game.name}</h3>
            {game.is_custom && (
              <span className="text-xs px-2 py-0.5 bg-blue-500/20 text-blue-400 rounded">Custom</span>
            )}
          </div>
          <p className="text-sm text-slate-400 truncate">{game.description}</p>
          <div className="flex items-center gap-3 mt-2 text-xs text-slate-500">
            <span>Port: {defaultPort}</span>
            <span>RAM: {game.recommended_ram_mb}MB</span>
          </div>
        </div>
        <button onClick={onToggle} className="p-2 hover:bg-slate-700 rounded-lg">
          {expanded ? <ChevronUp size={18} /> : <ChevronDown size={18} />}
        </button>
      </div>

      {expanded && (
        <div className="mt-4 pt-4 border-t border-slate-700">
          <div className="space-y-2 text-sm">
            <div className="flex items-center justify-between">
              <span className="text-slate-400">Docker Image</span>
              <code className="text-xs bg-slate-700 px-2 py-1 rounded">{game.docker_image}</code>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-slate-400">Volume Path</span>
              <code className="text-xs bg-slate-700 px-2 py-1 rounded">{game.volume_path}</code>
            </div>
            {game.stop_command && (
              <div className="flex items-center justify-between">
                <span className="text-slate-400">Stop Command</span>
                <code className="text-xs bg-slate-700 px-2 py-1 rounded">{game.stop_command}</code>
              </div>
            )}
            {game.install_image && (
              <div className="flex items-center justify-between">
                <span className="text-slate-400">Install Image</span>
                <code className="text-xs bg-slate-700 px-2 py-1 rounded">{game.install_image}</code>
              </div>
            )}
            {game.variables.length > 0 && (
              <div>
                <span className="text-slate-400">Variables:</span>
                <div className="mt-1 space-y-1">
                  {game.variables.slice(0, 5).map((v, i) => (
                    <div key={i} className="flex items-center gap-2 text-xs">
                      <code className="bg-slate-700 px-1 rounded">{v.env}</code>
                      <span className="text-slate-500">=</span>
                      <code className="bg-slate-700 px-1 rounded truncate">{v.default}</code>
                      {v.system_mapping && v.system_mapping !== 'none' && (
                        <span className="px-1 py-0.5 bg-blue-500/20 text-blue-300 rounded text-xs">{v.system_mapping}</span>
                      )}
                    </div>
                  ))}
                  {game.variables.length > 5 && <div className="text-slate-500 text-xs">+{game.variables.length - 5} more...</div>}
                </div>
              </div>
            )}
            {game.config_files.length > 0 && (
              <div>
                <span className="text-slate-400">Config Files:</span>
                <div className="mt-1 space-y-1">
                  {game.config_files.map((cfg, i) => (
                    <div key={i} className="flex items-center gap-2 text-xs">
                      <code className="bg-slate-700 px-1 rounded">{cfg.path}</code>
                      <span className="px-1 py-0.5 bg-cyan-500/20 text-cyan-300 rounded uppercase">{cfg.format}</span>
                      <span className="text-slate-500">({Object.keys(cfg.variables).length} vars)</span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
          
          <div className="flex items-center gap-2 mt-4">
            <button onClick={onExport} className="btn btn-secondary text-sm">
              {copied ? <Check size={14} className="text-emerald-500" /> : <Copy size={14} />}
              {copied ? 'Copied!' : 'Copy JSON'}
            </button>
            <button onClick={onEdit} className="btn btn-secondary text-sm">
              <Edit2 size={14} /> Edit
            </button>
            {game.is_custom && onDelete && (
              <button onClick={onDelete} className="btn btn-danger text-sm">
                <Trash2 size={14} /> Delete
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
