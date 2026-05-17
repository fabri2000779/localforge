import { useEffect, useState } from 'react';
import {
  Plus,
  RefreshCw,
  Trash2,
  Server,
  Cloud,
  Wifi,
  Cpu,
  HardDrive,
  MemoryStick,
} from 'lucide-react';
import { useNodesStore } from '../stores/nodesStore';
import type { NodeRecord, NodeStats } from '../types';
import { AddNodeWizard } from '../components/AddNodeWizard';

export function NodesPage() {
  const {
    nodes,
    nodeStats,
    fetchNodes,
    fetchNodeStats,
    removeNode,
    reconnectNode,
    isLoading,
  } = useNodesStore();
  const [wizardOpen, setWizardOpen] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);

  useEffect(() => {
    fetchNodes();
  }, [fetchNodes]);

  // Auto-refresh host stats for every known node every 5s.
  useEffect(() => {
    if (nodes.length === 0) return;
    const refresh = () => nodes.forEach((n) => fetchNodeStats(n.id));
    refresh();
    const interval = setInterval(refresh, 5_000);
    return () => clearInterval(interval);
  }, [nodes, fetchNodeStats]);

  const handleRemove = async (node: NodeRecord) => {
    if (node.kind.kind === 'local') return;
    if (
      !window.confirm(
        `Remove "${node.label}"? Its persisted token will be deleted; the agent on the server keeps running until you stop it manually.`,
      )
    ) {
      return;
    }
    setBusyId(node.id);
    try {
      await removeNode(node.id);
    } finally {
      setBusyId(null);
    }
  };

  const handleReconnect = async (node: NodeRecord) => {
    setBusyId(node.id);
    try {
      await reconnectNode(node.id);
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="animate-fade-in max-w-4xl">
      <header className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-3xl font-bold">Nodes</h1>
          <p className="text-slate-400 mt-2">
            Local Docker plus any remote LocalForge agents you've paired.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            className="btn btn-secondary text-sm"
            onClick={fetchNodes}
            disabled={isLoading}
            title="Refresh"
          >
            <RefreshCw
              size={16}
              className={isLoading ? 'animate-spin' : undefined}
            />
            Refresh
          </button>
          <button
            className="btn btn-primary text-sm"
            onClick={() => setWizardOpen(true)}
          >
            <Plus size={16} /> Add node
          </button>
        </div>
      </header>

      <div className="space-y-3">
        {nodes.length === 0 ? (
          <div className="card text-center py-12">
            <Server size={36} className="mx-auto text-slate-600 mb-3" />
            <h2 className="text-lg font-semibold mb-1">No nodes yet</h2>
            <p className="text-sm text-slate-400">
              The local Docker daemon will show up here once it's reachable.
            </p>
          </div>
        ) : (
          nodes.map((node) => (
            <NodeCard
              key={node.id}
              node={node}
              stats={nodeStats[node.id]}
              busy={busyId === node.id}
              onRemove={() => handleRemove(node)}
              onReconnect={() => handleReconnect(node)}
            />
          ))
        )}
      </div>

      <AddNodeWizard
        isOpen={wizardOpen}
        onClose={() => setWizardOpen(false)}
        onComplete={fetchNodes}
      />
    </div>
  );
}

function NodeCard({
  node,
  stats,
  busy,
  onRemove,
  onReconnect,
}: {
  node: NodeRecord;
  stats?: NodeStats;
  busy: boolean;
  onRemove: () => void;
  onReconnect: () => void;
}) {
  const isLocal = node.kind.kind === 'local';
  const url = !isLocal && node.kind.kind === 'remote' ? node.kind.url : null;
  const fingerprint =
    !isLocal && node.kind.kind === 'remote' ? node.kind.fingerprint : null;

  const memPct =
    stats && stats.memory_total_bytes > 0
      ? (stats.memory_used_bytes / stats.memory_total_bytes) * 100
      : null;
  const diskPct =
    stats && stats.disk_total_bytes > 0
      ? (stats.disk_used_bytes / stats.disk_total_bytes) * 100
      : null;

  return (
    <div className="card">
      <div className="flex items-start justify-between gap-4">
        <div className="flex items-start gap-4 flex-1 min-w-0">
          <div className="p-3 rounded-lg bg-slate-800">
            {isLocal ? (
              <Server className="text-blue-400" size={22} />
            ) : (
              <Cloud className="text-purple-400" size={22} />
            )}
          </div>
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2">
              <h3 className="font-semibold">{node.label}</h3>
              <span className="text-xs text-slate-500">({node.id})</span>
              {!stats && (
                <span className="text-xs text-amber-500 flex items-center gap-1">
                  <span className="w-1.5 h-1.5 rounded-full bg-amber-500" />
                  offline
                </span>
              )}
            </div>
            {url && (
              <div className="text-xs text-slate-400 mt-1 font-mono break-all">
                {url}
              </div>
            )}
            {fingerprint && (
              <div className="text-xs text-slate-600 mt-1 font-mono break-all">
                cert pinned: {fingerprint}
              </div>
            )}
            {isLocal && (
              <div className="text-xs text-slate-500 mt-1">
                Your Docker daemon
              </div>
            )}
          </div>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          {!isLocal && (
            <button
              className="btn-icon"
              onClick={onReconnect}
              disabled={busy}
              title="Reconnect"
            >
              {busy ? (
                <RefreshCw size={16} className="animate-spin" />
              ) : (
                <Wifi size={16} />
              )}
            </button>
          )}
          {!isLocal && (
            <button
              className="btn-icon hover:text-red-400"
              onClick={onRemove}
              disabled={busy}
              title="Remove"
            >
              <Trash2 size={16} />
            </button>
          )}
        </div>
      </div>

      {/* Host stats bars — only when the node is reachable. */}
      {stats && (
        <div className="grid grid-cols-1 md:grid-cols-3 gap-3 mt-4 pt-4 border-t border-slate-800">
          <StatBar
            icon={<Cpu size={14} className="text-blue-400" />}
            label={`CPU · ${stats.cpu_count} cores`}
            value={stats.cpu_percent}
            display={`${stats.cpu_percent.toFixed(1)}%`}
            colorClass="bg-blue-500"
          />
          <StatBar
            icon={<MemoryStick size={14} className="text-emerald-400" />}
            label={`Memory${
              stats.swap_total_bytes > 0
                ? ` · swap ${humanBytes(stats.swap_used_bytes)}/${humanBytes(stats.swap_total_bytes)}`
                : ''
            }`}
            value={memPct ?? 0}
            display={`${humanBytes(stats.memory_used_bytes)} / ${humanBytes(stats.memory_total_bytes)}`}
            colorClass={memPctClass(memPct)}
          />
          <StatBar
            icon={<HardDrive size={14} className="text-purple-400" />}
            label="Disk (data root)"
            value={diskPct ?? 0}
            display={`${humanBytes(stats.disk_used_bytes)} / ${humanBytes(stats.disk_total_bytes)}`}
            colorClass={memPctClass(diskPct)}
          />
        </div>
      )}

      {stats && stats.uptime_secs > 0 && (
        <div className="mt-2 text-xs text-slate-500 flex items-center gap-3">
          <span>uptime {humanDuration(stats.uptime_secs)}</span>
          {stats.load_avg_1m !== null && (
            <span>load {stats.load_avg_1m.toFixed(2)}</span>
          )}
        </div>
      )}
    </div>
  );
}

function StatBar({
  icon,
  label,
  value,
  display,
  colorClass,
}: {
  icon: React.ReactNode;
  label: string;
  value: number;
  display: string;
  colorClass: string;
}) {
  const pct = Math.max(0, Math.min(100, value));
  return (
    <div>
      <div className="flex items-center gap-2 mb-1 text-xs text-slate-400">
        {icon}
        <span className="truncate">{label}</span>
      </div>
      <div className="h-1.5 bg-slate-800 rounded overflow-hidden">
        <div
          className={`h-full ${colorClass} transition-all`}
          style={{ width: `${pct}%` }}
        />
      </div>
      <div className="text-xs text-slate-500 mt-1 font-mono">{display}</div>
    </div>
  );
}

function memPctClass(pct: number | null): string {
  if (pct === null) return 'bg-slate-700';
  if (pct >= 90) return 'bg-red-500';
  if (pct >= 75) return 'bg-amber-500';
  return 'bg-emerald-500';
}

function humanBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  if (n < 1024 * 1024 * 1024 * 1024)
    return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  return `${(n / (1024 * 1024 * 1024 * 1024)).toFixed(2)} TB`;
}

function humanDuration(secs: number): string {
  const days = Math.floor(secs / 86400);
  const hours = Math.floor((secs % 86400) / 3600);
  const minutes = Math.floor((secs % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m`;
  return `${secs}s`;
}
