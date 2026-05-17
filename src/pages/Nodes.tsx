import { useEffect, useState } from 'react';
import { Plus, RefreshCw, Trash2, Server, Cloud, Wifi, WifiOff } from 'lucide-react';
import { useNodesStore } from '../stores/nodesStore';
import type { NodeRecord } from '../types';
import { AddNodeWizard } from '../components/AddNodeWizard';

export function NodesPage() {
  const { nodes, fetchNodes, removeNode, reconnectNode, isLoading } =
    useNodesStore();
  const [wizardOpen, setWizardOpen] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);

  useEffect(() => {
    fetchNodes();
  }, [fetchNodes]);

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
  busy,
  onRemove,
  onReconnect,
}: {
  node: NodeRecord;
  busy: boolean;
  onRemove: () => void;
  onReconnect: () => void;
}) {
  const isLocal = node.kind.kind === 'local';
  const url = !isLocal && node.kind.kind === 'remote' ? node.kind.url : null;
  const fingerprint =
    !isLocal && node.kind.kind === 'remote' ? node.kind.fingerprint : null;

  return (
    <div className="card flex items-start justify-between gap-4">
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
        {isLocal && (
          <span className="text-xs text-slate-500 flex items-center gap-1">
            <WifiOff size={14} className="opacity-0" />
          </span>
        )}
      </div>
    </div>
  );
}
