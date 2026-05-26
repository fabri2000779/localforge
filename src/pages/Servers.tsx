import { useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Plus, RefreshCw, Server, Cloud } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { useServerStore } from '../stores/serverStore';
import { useNodesStore } from '../stores/nodesStore';
import {
  useCanAct,
  useDisplayedServers,
  useIsSubUser,
  useServerMachineInfo,
} from '../utils/subUser';
import { useOwnerFleet, type FleetEntry } from '../hooks/useFleet';
import { ServerCard } from '../components/ServerCard';
import type { Server as ServerType } from '../types';

const ALL = '__all__';

type MachineKind = 'desktop' | 'agent' | 'local' | 'unknown';

interface Item {
  server: ServerType;
  machineId: string;
  machineName: string;
  machineKind: MachineKind;
  onOpen?: () => void;
  onAction?: (action: 'start' | 'stop' | 'delete') => void;
}

export function Servers() {
  const navigate = useNavigate();
  const isSubUser = useIsSubUser();
  const nodes = useNodesStore((s) => s.nodes);
  const setActiveNode = useNodesStore((s) => s.setActiveNode);

  // Owner with more than one paired node → aggregate every machine's
  // servers into one cross-machine view (matches the mobile app). Solo
  // owners and sub-users fall through to the single-source path below.
  const useOwner = !isSubUser && nodes.length >= 2;
  const ownerFleet = useOwnerFleet(useOwner);

  // The single-source list: owner → local Docker; sub-user → the owner's
  // cloud-synced servers (decrypted, live status from relay discovery).
  const displayed = useDisplayedServers();
  const subMachineInfo = useServerMachineInfo();
  const { isLoading, fetchServers } = useServerStore();
  // Sub-users below admin role can't create servers on the owner's host;
  // hide the CTA for them. Always true for the owner / local mode.
  const canCreate = useCanAct('server.create');

  const [filter, setFilter] = useState<string>(ALL);

  // Node-targeted action for the owner fleet: act on the server's OWN node
  // (not the globally-active one), then refresh so the badge catches up.
  function ownerAction(e: FleetEntry, action: 'start' | 'stop' | 'delete') {
    const cmd =
      action === 'start'
        ? 'start_server'
        : action === 'stop'
          ? 'stop_server'
          : 'delete_server';
    const args: Record<string, unknown> =
      action === 'delete'
        ? { serverId: e.server.id, nodeId: e.machineId, deleteData: false }
        : { serverId: e.server.id, nodeId: e.machineId };
    void invoke(cmd, args)
      .catch(() => {})
      .finally(() => setTimeout(() => ownerFleet.refresh(), 800));
  }

  const items = useMemo<Item[]>(() => {
    if (useOwner) {
      return ownerFleet.entries.map((e) => ({
        server: e.server,
        machineId: e.machineId,
        machineName: e.machineName,
        machineKind: e.machineKind,
        // Open detail by first focusing the server's node so the (active-
        // node-scoped) detail screen resolves it.
        onOpen: () => {
          setActiveNode(e.machineId);
          navigate(`/servers/${e.server.id}`);
        },
        onAction: (a) => ownerAction(e, a),
      }));
    }
    // Owner-solo or sub-user: default ServerCard routing (serverStore /
    // relay) + default navigation works as-is.
    return displayed.map((s) => {
      const m = subMachineInfo[s.id];
      return {
        server: s,
        machineId: m?.machineId ?? '',
        machineName: m?.machineName ?? '',
        machineKind: m?.machineKind ?? 'unknown',
      };
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [useOwner, ownerFleet.entries, displayed, subMachineInfo]);

  // Distinct machines hosting a visible server, stable order.
  const machines = useMemo(() => {
    const seen = new Map<string, { id: string; name: string; kind: MachineKind }>();
    for (const it of items) {
      if (it.machineId && !seen.has(it.machineId)) {
        seen.set(it.machineId, { id: it.machineId, name: it.machineName, kind: it.machineKind });
      }
    }
    return [...seen.values()].sort((a, b) => a.name.localeCompare(b.name));
  }, [items]);

  // Group-by-machine chips only earn their place once there are 2+ machines.
  const showChips = machines.length >= 2;

  const visible = useMemo(() => {
    if (!showChips || filter === ALL) return items;
    return items.filter((it) => it.machineId === filter);
  }, [items, filter, showChips]);

  function refresh() {
    if (useOwner) ownerFleet.refresh();
    else fetchServers();
  }

  return (
    <div className="animate-fade-in">
      <header className="page-header flex items-end justify-between gap-4 flex-wrap">
        <div>
          <div className="eyebrow mb-2">Library</div>
          <h1 className="page-title">My Servers</h1>
          <p className="page-subtitle">
            Manage all your game servers in one place.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={refresh}
            disabled={isLoading || ownerFleet.loading}
            className="btn btn-secondary"
            aria-label="Refresh"
          >
            <RefreshCw
              size={15}
              className={isLoading || ownerFleet.loading ? 'animate-spin' : ''}
            />
          </button>
          {canCreate && (
            <button
              onClick={() => navigate('/servers/create')}
              className="btn btn-primary"
            >
              <Plus size={15} strokeWidth={2.2} />
              Create Server
            </button>
          )}
        </div>
      </header>

      {/* Machine filter chips — appear only when servers span 2+ machines.
          Mirrors the mobile app's group-by-machine switcher. */}
      {showChips && (
        <div className="flex items-center gap-2 flex-wrap mb-5">
          <FilterChip
            active={filter === ALL}
            onClick={() => setFilter(ALL)}
            label="All machines"
          />
          {machines.map((m) => (
            <FilterChip
              key={m.id}
              active={filter === m.id}
              onClick={() => setFilter(m.id)}
              label={m.name}
              icon={m.kind === 'agent' ? <Cloud size={12} /> : <Server size={12} />}
            />
          ))}
        </div>
      )}

      {visible.length === 0 ? (
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
            {canCreate
              ? "You haven't created any servers. Get started by creating your first one."
              : 'No servers have been shared with you yet. Ask the owner to add one.'}
          </p>
          {canCreate && (
            <button
              onClick={() => navigate('/servers/create')}
              className="btn btn-primary"
            >
              <Plus size={15} strokeWidth={2.2} />
              Create your first server
            </button>
          )}
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {visible.map((it) => (
            <ServerCard
              key={it.server.id}
              server={it.server}
              machineLabel={showChips ? it.machineName : undefined}
              machineKind={it.machineKind}
              onOpen={it.onOpen}
              onAction={it.onAction}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function FilterChip({
  active,
  onClick,
  label,
  icon,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  icon?: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={`inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full text-[12.5px] font-medium border transition-colors ${
        active
          ? 'bg-blue-500/15 border-blue-500/40 text-blue-200'
          : 'bg-slate-800/60 border-slate-700/60 text-slate-400 hover:text-slate-200 hover:bg-slate-800'
      }`}
    >
      {icon}
      {label}
    </button>
  );
}
