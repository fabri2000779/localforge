import { useEffect, useState } from 'react';
import {
  Plus,
  RefreshCw,
  Trash2,
  Server,
  Cloud,
  CloudOff,
  Wifi,
  Cpu,
  HardDrive,
  MemoryStick,
  Link2,
  Copy,
  X,
  Pencil,
  Check,
} from 'lucide-react';
import { useNodesStore, type CloudNodeSummary } from '../stores/nodesStore';
import { useAuthStore } from '../stores/authStore';
import type { NodeRecord, NodeStats } from '../types';
import { AddNodeWizard } from '../components/AddNodeWizard';

/** Agent-node caps per plan — mirrors the cloud's NODE_CAP. Counts
 *  cloud-linked agents only; desktop installs are unlimited. */
const NODE_CAP: Record<string, number> = { free: 0, hobby: 2, team: 10 };

export function NodesPage() {
  const {
    nodes,
    nodeStats,
    cloudNodes,
    fetchNodes,
    fetchNodeStats,
    fetchCloudNodes,
    linkNodeToCloud,
    revokeCloudNode,
    removeNode,
    reconnectNode,
    renameMachine,
    cloudMachines,
    fetchMachines,
    isLoading,
  } = useNodesStore();
  const [wizardOpen, setWizardOpen] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  // The `localforge-agent link <blob>` command shown after enrolling a node.
  const [linkCmd, setLinkCmd] = useState<string | null>(null);

  // Agent-node usage vs the plan cap, so paid users see their limit clearly.
  const plan = useAuthStore((s) => s.me?.subscription.plan ?? 'free');
  const nodeCap = NODE_CAP[plan] ?? 0;
  const usedNodes = cloudNodes.filter((cn) => !cn.revoked).length;
  const atCap = nodeCap > 0 && usedNodes >= nodeCap;

  useEffect(() => {
    fetchNodes();
    fetchCloudNodes();
    fetchMachines();
  }, [fetchNodes, fetchCloudNodes, fetchMachines]);

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
    const linked = cloudNodes.find((cn) => cn.id === node.id && !cn.revoked);
    if (
      !window.confirm(
        linked
          ? `Remove "${node.label}"? It's linked to the cloud relay, so this also unlinks it — mobile / other desktops lose direct access and the node slot is freed. The agent on the server keeps running until you stop it manually.`
          : `Remove "${node.label}"? Its persisted token will be deleted; the agent on the server keeps running until you stop it manually.`,
      )
    ) {
      return;
    }
    setBusyId(node.id);
    try {
      // Cascade: unlink from the cloud FIRST (hard-deletes the enrollment +
      // frees the cap slot + drops the relay socket). Doing it before the
      // local removal means a failure here aborts cleanly instead of leaving
      // an orphaned cloud node with no UI left to unlink it.
      if (linked) {
        await revokeCloudNode(node.id);
      }
      await removeNode(node.id);
    } catch (e) {
      window.alert(`Couldn't remove node: ${e}`);
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

  const handleLink = async (node: NodeRecord) => {
    setBusyId(node.id);
    try {
      const res = await linkNodeToCloud(node.id, node.label);
      setLinkCmd(`localforge-agent link ${res.enrollmentBlob}`);
    } catch (e) {
      window.alert(`Couldn't link to cloud: ${e}`);
    } finally {
      setBusyId(null);
    }
  };

  const handleUnlink = async (node: NodeRecord) => {
    if (
      !window.confirm(
        `Unlink "${node.label}" from the cloud relay? Mobile / other desktops won't control it directly until you re-link. The agent keeps running.`,
      )
    ) {
      return;
    }
    setBusyId(node.id);
    try {
      await revokeCloudNode(node.id);
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="animate-fade-in max-w-4xl">
      <header className="page-header flex items-end justify-between gap-4 flex-wrap">
        <div>
          <div className="eyebrow mb-2">Infrastructure</div>
          <h1 className="page-title">Nodes</h1>
          <p className="page-subtitle">
            Local Docker plus any remote LocalForge agents you&apos;ve paired.
          </p>
          {plan !== 'free' && (
            <p className="text-[12px] text-slate-500 mt-1.5">
              Cloud-linked agent nodes:{' '}
              <span className={`font-medium ${atCap ? 'text-amber-400' : 'text-slate-300'}`}>
                {usedNodes} of {nodeCap}
              </span>{' '}
              used{atCap ? ' — limit reached' : ''}. Desktop installs don&apos;t count.
            </p>
          )}
        </div>
        <div className="flex items-center gap-2">
          <button
            className="btn btn-secondary btn-sm"
            onClick={fetchNodes}
            disabled={isLoading}
            title="Refresh"
          >
            <RefreshCw
              size={13}
              className={isLoading ? 'animate-spin' : undefined}
            />
            Refresh
          </button>
          <button
            className="btn btn-primary btn-sm"
            onClick={() => setWizardOpen(true)}
          >
            <Plus size={13} strokeWidth={2.2} /> Add node
          </button>
        </div>
      </header>

      <div className="space-y-3">
        {nodes.length === 0 ? (
          <div className="card card-elevated text-center py-12 flex flex-col items-center">
            <div className="w-12 h-12 rounded-xl bg-slate-800/70 inline-flex items-center justify-center mb-4">
              <Server size={22} className="text-slate-500" />
            </div>
            <h2 className="text-base font-semibold text-slate-100 mb-1">
              No nodes yet
            </h2>
            <p className="text-sm text-slate-400 max-w-sm">
              The local Docker daemon will show up here once it&apos;s
              reachable.
            </p>
          </div>
        ) : (
          nodes.map((node) => (
            <NodeCard
              key={node.id}
              node={node}
              stats={nodeStats[node.id]}
              cloudNode={cloudNodes.find((cn) => cn.id === node.id && !cn.revoked)}
              busy={busyId === node.id}
              onRemove={() => handleRemove(node)}
              onReconnect={() => handleReconnect(node)}
              onLink={() => handleLink(node)}
              onUnlink={() => handleUnlink(node)}
              onRename={
                node.kind.kind === 'local' ? renameMachine : undefined
              }
            />
          ))
        )}
      </div>

      {cloudMachines.length > 0 && (
        <section className="card mt-5">
          <div className="section-title mb-1">
            <Cloud size={15} className="text-purple-400" />
            Machines in this organization
          </div>
          <p className="text-xs text-slate-500 mb-3">
            Every machine you (and your team) can reach over the cloud relay —
            desktops and VPS agents, wherever they are.
          </p>
          <div className="space-y-1.5">
            {cloudMachines.map((m) => (
              <div
                key={m.id}
                className="flex items-center gap-2.5 px-2 py-1.5 rounded-md hover:bg-slate-800/50"
              >
                <div
                  className={`shrink-0 w-7 h-7 rounded-md flex items-center justify-center ${
                    m.kind === 'desktop' ? 'bg-blue-500/15' : 'bg-purple-500/15'
                  }`}
                >
                  {m.kind === 'desktop' ? (
                    <Server size={13} className="text-blue-400" />
                  ) : (
                    <Cloud size={13} className="text-purple-400" />
                  )}
                </div>
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium text-slate-100 truncate">
                    {m.name}
                  </div>
                  <div className="text-[11px] text-slate-500 capitalize">
                    {m.kind}
                  </div>
                </div>
                <span
                  className={`flex items-center gap-1 text-[11px] ${
                    m.online ? 'text-emerald-400' : 'text-slate-500'
                  }`}
                >
                  <span
                    className={`w-1.5 h-1.5 rounded-full ${
                      m.online ? 'bg-emerald-400' : 'bg-slate-600'
                    }`}
                  />
                  {m.online ? 'online' : 'offline'}
                </span>
              </div>
            ))}
          </div>
        </section>
      )}

      <AddNodeWizard
        isOpen={wizardOpen}
        onClose={() => setWizardOpen(false)}
        onComplete={fetchNodes}
      />

      {linkCmd && (
        <LinkCommandDialog command={linkCmd} onClose={() => setLinkCmd(null)} />
      )}
    </div>
  );
}

/** Shows the one-time `localforge-agent link <blob>` command after enrolling
 *  a node. The operator runs it on the VPS (then restarts the agent) and the
 *  agent connects to the relay. Shown once — the token isn't recoverable. */
function LinkCommandDialog({
  command,
  onClose,
}: {
  command: string;
  onClose: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      /* clipboard blocked — the user can select manually */
    }
  };
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4"
      onClick={onClose}
    >
      <div
        className="card card-elevated max-w-lg w-full"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-4 mb-3">
          <div>
            <h2 className="text-base font-semibold text-slate-100">Link this node</h2>
            <p className="text-sm text-slate-400 mt-1">
              Run this on the VPS, then restart the agent. It&apos;ll connect to
              the relay so you can control it from your phone with this desktop
              closed. Shown once — copy it now.
            </p>
          </div>
          <button className="btn-icon" onClick={onClose} aria-label="Close">
            <X size={16} />
          </button>
        </div>
        <div className="flex items-stretch gap-2">
          <code className="flex-1 text-xs bg-slate-900/80 border border-slate-700 rounded-lg p-3 font-mono break-all text-slate-300 max-h-32 overflow-auto">
            {command}
          </code>
          <button className="btn btn-secondary btn-sm shrink-0 self-start" onClick={copy}>
            <Copy size={13} /> {copied ? 'Copied' : 'Copy'}
          </button>
        </div>
      </div>
    </div>
  );
}

function NodeCard({
  node,
  stats,
  cloudNode,
  busy,
  onRemove,
  onReconnect,
  onLink,
  onUnlink,
  onRename,
}: {
  node: NodeRecord;
  stats?: NodeStats;
  cloudNode?: CloudNodeSummary;
  busy: boolean;
  onRemove: () => void;
  onReconnect: () => void;
  onLink: () => void;
  onUnlink: () => void;
  /** Only supplied for the local node — renames THIS machine. */
  onRename?: (name: string) => Promise<void>;
}) {
  const isLocal = node.kind.kind === 'local';
  // Inline machine-rename state (local node only).
  const [editingName, setEditingName] = useState(false);
  const [draftName, setDraftName] = useState(node.label);
  const [savingName, setSavingName] = useState(false);

  const beginRename = () => {
    setDraftName(node.label);
    setEditingName(true);
  };
  const commitRename = async () => {
    const next = draftName.trim();
    if (!onRename || !next || next === node.label) {
      setEditingName(false);
      return;
    }
    setSavingName(true);
    try {
      await onRename(next);
      setEditingName(false);
    } catch (e) {
      console.error('[Nodes] rename machine:', e);
    } finally {
      setSavingName(false);
    }
  };
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
              {editingName ? (
                <>
                  <input
                    autoFocus
                    value={draftName}
                    onChange={(e) => setDraftName(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') void commitRename();
                      if (e.key === 'Escape') setEditingName(false);
                    }}
                    disabled={savingName}
                    maxLength={40}
                    className="bg-slate-800 border border-slate-600 rounded px-2 py-0.5 text-sm font-semibold text-slate-100 outline-none focus:border-blue-500 w-44 min-w-0"
                    aria-label="Machine name"
                  />
                  <button
                    className="btn-icon text-emerald-400"
                    onClick={() => void commitRename()}
                    disabled={savingName}
                    title="Save name"
                  >
                    <Check size={14} />
                  </button>
                  <button
                    className="btn-icon"
                    onClick={() => setEditingName(false)}
                    disabled={savingName}
                    title="Cancel"
                  >
                    <X size={14} />
                  </button>
                </>
              ) : (
                <>
                  <h3 className="font-semibold">{node.label}</h3>
                  {onRename && (
                    <button
                      className="btn-icon text-slate-500 hover:text-slate-300"
                      onClick={beginRename}
                      title="Rename this machine"
                    >
                      <Pencil size={12} />
                    </button>
                  )}
                  {!isLocal && (
                    <span className="text-xs text-slate-500">({node.id})</span>
                  )}
                  {!stats && (
                    <span className="text-xs text-amber-500 flex items-center gap-1">
                      <span className="w-1.5 h-1.5 rounded-full bg-amber-500" />
                      offline
                    </span>
                  )}
                </>
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
            {!isLocal && cloudNode && (
              <div className="text-xs mt-1 flex items-center gap-1">
                {cloudNode.online ? (
                  <Cloud size={11} className="text-emerald-400" />
                ) : (
                  <CloudOff size={11} className="text-slate-500" />
                )}
                <span
                  className={cloudNode.online ? 'text-emerald-400' : 'text-slate-500'}
                >
                  Cloud relay {cloudNode.online ? 'online' : 'offline'}
                </span>
              </div>
            )}
          </div>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          {!isLocal && (
            <button
              className={`btn-icon ${cloudNode ? 'text-emerald-400 hover:text-red-400' : ''}`}
              onClick={cloudNode ? onUnlink : onLink}
              disabled={busy}
              title={
                cloudNode
                  ? 'Unlink from cloud relay'
                  : 'Link to cloud relay — control from your phone with this desktop closed'
              }
            >
              {cloudNode ? <Cloud size={16} /> : <Link2 size={16} />}
            </button>
          )}
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
