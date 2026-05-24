import { useEffect, useRef, useState } from 'react';
import { ChevronDown, Server, Cloud, Check, Plus } from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { listen } from '@tauri-apps/api/event';
import { useNodesStore } from '../stores/nodesStore';

/**
 * Compact dropdown at the top of the sidebar. Shows the active node and,
 * when expanded, the list of all paired nodes so the user can switch.
 * Switching kicks off a re-fetch in serverStore and dockerStore (they
 * subscribe to nodesStore.activeNodeId).
 */
export function NodeSelector() {
  const { nodes, activeNodeId, setActiveNode, fetchNodes } = useNodesStore();
  const [open, setOpen] = useState(false);
  const navigate = useNavigate();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    fetchNodes();
    // The backend loads persisted remote nodes asynchronously at startup
    // (after Docker connects), so this initial fetch can race ahead of them
    // and show only the local node until you open the Nodes page. Re-fetch
    // when the backend signals remotes are loaded.
    const un = listen('nodes-changed', () => {
      void fetchNodes();
    });
    return () => {
      void un.then((f) => f());
    };
  }, [fetchNodes]);

  // Click-outside to close.
  useEffect(() => {
    if (!open) return;
    const onClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener('mousedown', onClick);
    return () => document.removeEventListener('mousedown', onClick);
  }, [open]);

  const active =
    nodes.find((n) => n.id === activeNodeId) ??
    nodes.find((n) => n.kind.kind === 'local');

  return (
    <div ref={ref} className="relative">
      <div className="text-[10px] uppercase tracking-wider text-slate-500 font-semibold mb-1.5 pl-1">
        Active node
      </div>
      <button
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center gap-2.5 px-3 py-2.5 rounded-lg bg-slate-800/70 hover:bg-slate-800 border border-slate-700/60 transition-colors text-left"
      >
        <div
          className={`shrink-0 w-7 h-7 rounded-md flex items-center justify-center ${
            active?.kind.kind === 'local' ? 'bg-blue-500/15' : 'bg-purple-500/15'
          }`}
        >
          {active?.kind.kind === 'local' ? (
            <Server size={14} className="text-blue-400" />
          ) : (
            <Cloud size={14} className="text-purple-400" />
          )}
        </div>
        <div className="flex-1 min-w-0">
          <div className="text-sm font-medium truncate text-slate-100">
            {active?.label ?? 'Loading…'}
          </div>
          <div className="text-[11px] text-slate-500 truncate">
            {active?.kind.kind === 'remote'
              ? active.kind.url
              : 'Local Docker daemon'}
          </div>
        </div>
        <ChevronDown
          size={14}
          className={`text-slate-500 transition-transform shrink-0 ${
            open ? 'rotate-180' : ''
          }`}
        />
      </button>

      {open && (
        <div className="absolute left-0 right-0 top-full mt-1.5 z-30 bg-slate-900 border border-slate-700 rounded-lg shadow-2xl py-1 overflow-hidden">
          {nodes.map((node) => {
            const isActive = node.id === activeNodeId;
            const isLocal = node.kind.kind === 'local';
            return (
              <button
                key={node.id}
                onClick={() => {
                  setActiveNode(node.id);
                  setOpen(false);
                }}
                className={`w-full flex items-center gap-2.5 px-3 py-2 text-left text-sm hover:bg-slate-800 ${
                  isActive ? 'bg-slate-800/60' : ''
                }`}
              >
                {isLocal ? (
                  <Server size={14} className="text-blue-400 shrink-0" />
                ) : (
                  <Cloud size={14} className="text-purple-400 shrink-0" />
                )}
                <div className="flex-1 min-w-0">
                  <div className="font-medium truncate">{node.label}</div>
                  {node.kind.kind === 'remote' && (
                    <div className="text-xs text-slate-500 truncate font-mono">
                      {node.kind.url}
                    </div>
                  )}
                </div>
                {isActive && (
                  <Check size={14} className="text-emerald-400 shrink-0" />
                )}
              </button>
            );
          })}
          <div className="border-t border-slate-800 mt-1 pt-1">
            <button
              onClick={() => {
                setOpen(false);
                navigate('/nodes');
              }}
              className="w-full flex items-center gap-2 px-3 py-2 text-left text-sm text-slate-400 hover:bg-slate-800 hover:text-white"
            >
              <Plus size={14} />
              Manage nodes
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
